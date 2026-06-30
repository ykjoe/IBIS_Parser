use std::path::Path;
use std::fs;

mod ibis_parser;

use ibis_parser::core::ibis_file_parse;
use ibis_parser::ibis_structure::IBIS_File;

#[derive(Debug)]
enum IbisFileReadError {
    FileNotFound,
    ParserFailed,
}


fn ibis_file_read<P: AsRef<Path>>(path: P) -> Result<IBIS_File, IbisFileReadError> {
    let content = fs::read_to_string(&path)
        .map_err(|_| IbisFileReadError::FileNotFound)?;
    let ibis_file = ibis_file_parse(&content)
        .map_err(|_| IbisFileReadError::ParserFailed);

    ibis_file
}

fn main() {
    match ibis_file_read("tests/f103c8.ibs") {
        Ok(ibis_file) => println!("Parsed IBIS file: {:?}", ibis_file),
        Err(IbisFileReadError::FileNotFound) => eprintln!("Error: file not found"),
        Err(IbisFileReadError::ParserFailed) => eprintln!("Error: parser failed"),
    }
}
