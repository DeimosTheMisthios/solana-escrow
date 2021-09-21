use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_pack::{IsInitialized, Pack},
    pubkey::Pubkey,
    sysvar::{rent::Rent, Sysvar},
};  // default solana imports

use spl_token::state::Account as TokenAccount;  // solana token imports

use crate::{error::EscrowError, instruction::EscrowInstruction, state::Escrow};

// look at instruction.rs first
// two types of instructions -> InitEscrow, and Exchange
// InitEscrow has the requested accounts listed, and those are passed as accounts
pub struct Processor;
impl Processor {
    pub fn process(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        instruction_data: &[u8],
    ) -> ProgramResult {
        let instruction = EscrowInstruction::unpack(instruction_data)?; // either an instruction or failure

        match instruction {
            EscrowInstruction::InitEscrow { amount } => {
                msg!("Instruction: InitEscrow");
                Self::process_init_escrow(accounts, amount, program_id) // amount is unpacked by instruction.rs
            }
            EscrowInstruction::Exchange { amount } => {
                msg!("Instruction: Exchange");
                Self::process_exchange(accounts, amount, program_id)
            }
        }
    }

    fn process_init_escrow(
        accounts: &[AccountInfo],
        amount: u64,
        program_id: &Pubkey,
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();   // iterable
        let initializer = next_account_info(account_info_iter)?;    // first account

        if !initializer.is_signer { // must be the signer
            return Err(ProgramError::MissingRequiredSignature);
        }

        let temp_token_account = next_account_info(account_info_iter)?; // this is the one whose ownership will be transferred
                                                                        // to escrow's pda_account
        let token_to_receive_account = next_account_info(account_info_iter)?;   // alice's Y token account
        if token_to_receive_account.owner != spl_token::id() { // should be owned by the token program
                                                                // note that this difference from "token account owner attribute"
                                                                // who is Alice
            return Err(ProgramError::IncorrectProgramId);
        }

        let escrow_account = next_account_info(account_info_iter)?; // state account
        let rent = &Rent::from_account_info(next_account_info(account_info_iter)?)?;

        if !rent.is_exempt(escrow_account.lamports(), escrow_account.data_len()) {  // state account must be rent exempt -> why ?
            return Err(EscrowError::NotRentExempt.into());
        }

        let mut escrow_info = Escrow::unpack_unchecked(&escrow_account.data.borrow())?; // unpack data stored at escrow account address
        if escrow_info.is_initialized() {   // if initialized => fuck off
            return Err(ProgramError::AccountAlreadyInitialized);
        }

        escrow_info.is_initialized = true;
        escrow_info.initializer_pubkey = *initializer.key;
        escrow_info.temp_token_account_pubkey = *temp_token_account.key;
        escrow_info.initializer_token_to_receive_account_pubkey = *token_to_receive_account.key;
        escrow_info.expected_amount = amount;

        Escrow::pack(escrow_info, &mut escrow_account.data.borrow_mut())?;  // store it at the address
        let (pda, _nonce) = Pubkey::find_program_address(&[b"escrow"], program_id); // PDA is owned by this program

        let token_program = next_account_info(account_info_iter)?;  // token program
        // use instruction to tell token program to change owner of temp_token_account
        // from Alice to pda_account
        let owner_change_ix = spl_token::instruction::set_authority(    // see spl_token API for params
            token_program.key,
            temp_token_account.key,
            Some(&pda),
            spl_token::instruction::AuthorityType::AccountOwner,
            initializer.key,
            &[&initializer.key],
        )?;

        msg!("Calling the token program to transfer token account ownership...");
        invoke(
            &owner_change_ix,
            &[
                temp_token_account.clone(), // again check API definition
                initializer.clone(),
                token_program.clone(),
            ],
        )?;

        Ok(())  // Ok() => return an empty Ok => () is an empty tuple
    }

    fn process_exchange(
        accounts: &[AccountInfo],
        amount_expected_by_taker: u64,
        program_id: &Pubkey,
    ) -> ProgramResult {    // if escrow is inited, here's how to take trade
        let account_info_iter = &mut accounts.iter();
        let taker = next_account_info(account_info_iter)?;  // taker / signer

        if !taker.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        let takers_sending_token_account = next_account_info(account_info_iter)?;   // Y token from Bob

        let takers_token_to_receive_account = next_account_info(account_info_iter)?;    // X token to Bob

        let pdas_temp_token_account = next_account_info(account_info_iter)?;    // this is the PDA account created for Alice's X tokens
                                                                                // not sure why it needs to be passed -> should be stored in state no?
        let pdas_temp_token_account_info =
            TokenAccount::unpack(&pdas_temp_token_account.data.borrow())?;
        let (pda, nonce) = Pubkey::find_program_address(&[b"escrow"], program_id);

        if amount_expected_by_taker != pdas_temp_token_account_info.amount {    // ensure no front running
            return Err(EscrowError::ExpectedAmountMismatch.into());
        }

        let initializers_main_account = next_account_info(account_info_iter)?;  // Alice's account for SOL?
        let initializers_token_to_receive_account = next_account_info(account_info_iter)?;  // Alice's Y token account
        let escrow_account = next_account_info(account_info_iter)?; // state account

        let escrow_info = Escrow::unpack(&escrow_account.data.borrow())?;

        // i don't know why so many checks below are needed -> if Bob passes state address
        // it should be his responsibility to check, not the program's (Ctrl F for "Bob can")
        // maybe front running prevention by Alice re-writing state?

        // weirdly we haven't checked that Bob is indeed sending the token that Alice has expected
            // mint of the token?
        if escrow_info.temp_token_account_pubkey != *pdas_temp_token_account.key {  // lol why ask in line 123 then
            return Err(ProgramError::InvalidAccountData);
        }

        if escrow_info.initializer_pubkey != *initializers_main_account.key {   // assert Alice trade to finish
            return Err(ProgramError::InvalidAccountData);
        }

        if escrow_info.initializer_token_to_receive_account_pubkey  // Alice's Y token account passed by Bob must == state
            != *initializers_token_to_receive_account.key
        {
            return Err(ProgramError::InvalidAccountData);
        }

        let token_program = next_account_info(account_info_iter)?;

        // transfer from Bob (context) to Alice
        let transfer_to_initializer_ix = spl_token::instruction::transfer(
            token_program.key,
            takers_sending_token_account.key,
            initializers_token_to_receive_account.key,
            taker.key,
            &[&taker.key],
            escrow_info.expected_amount,
        )?;
        msg!("Calling the token program to transfer tokens to the escrow's initializer...");
        invoke(
            &transfer_to_initializer_ix,
            &[
                takers_sending_token_account.clone(),
                initializers_token_to_receive_account.clone(),
                taker.clone(),
                token_program.clone(),
            ],
        )?;

        let pda_account = next_account_info(account_info_iter)?;

        // transfer Alice's escrowed money to Bob (owned by PDA so it needs to be signed by the program)
        let transfer_to_taker_ix = spl_token::instruction::transfer(
            token_program.key,
            pdas_temp_token_account.key,
            takers_token_to_receive_account.key,
            &pda,
            &[&pda],
            pdas_temp_token_account_info.amount,
        )?;
        msg!("Calling the token program to transfer tokens to the taker...");
        invoke_signed(
            &transfer_to_taker_ix,
            &[
                pdas_temp_token_account.clone(),
                takers_token_to_receive_account.clone(),
                pda_account.clone(),
                token_program.clone(),
            ],
            &[&[&b"escrow"[..], &[nonce]]],
        )?;

        // then close the PDA account, again via invoke_signed
        let close_pdas_temp_acc_ix = spl_token::instruction::close_account(
            token_program.key,
            pdas_temp_token_account.key,
            initializers_main_account.key,
            &pda,
            &[&pda],
        )?;
        msg!("Calling the token program to close pda's temp account...");
        invoke_signed(
            &close_pdas_temp_acc_ix,
            &[
                pdas_temp_token_account.clone(),
                initializers_main_account.clone(),
                pda_account.clone(),
                token_program.clone(),
            ],
            &[&[&b"escrow"[..], &[nonce]]],
        )?;

        // close the state account
        msg!("Closing the escrow account...");
        **initializers_main_account.lamports.borrow_mut() = initializers_main_account
            .lamports()
            .checked_add(escrow_account.lamports())
            .ok_or(EscrowError::AmountOverflow)?;
        **escrow_account.lamports.borrow_mut() = 0;
        *escrow_account.data.borrow_mut() = &mut [];

        Ok(())
    }
}
