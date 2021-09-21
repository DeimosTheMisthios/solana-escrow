use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, pubkey::Pubkey,
};

use crate::processor::Processor;

entrypoint!(process_instruction);   // first line, declares that the entrypoint is the function `process_instruction`
fn process_instruction(
    program_id: &Pubkey,            // the program_id is used to find the address of a temp public key (PDA), which is a program derived address
    accounts: &[AccountInfo],       // the accounts to play with
    instruction_data: &[u8],        // data to play with
) -> ProgramResult {
    Processor::process(program_id, accounts, instruction_data)  // call this in the processor file, avoid garbage here
}
