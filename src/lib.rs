pub mod error;
pub mod instruction;
pub mod processor;
pub mod state;

#[cfg(not(feature = "no-entrypoint"))]
pub mod entrypoint; // if no entrypoint cargo feature while adding dep, this line is not executed
                    // thanks to line above
