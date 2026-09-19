use std::io::{Read, Seek, SeekFrom};
use std::rc::Rc;
use super::{Columns, DataFile, DataFileHeader, ReadDataFileHeader};
use crate::{DatabaseError, RowID, TableChunk, TypeID, Value};


//trait for data to quickly add them to an u8 vector in little endian order
pub trait WriteLEBytes {
    fn write_bytes(&self, buffer: &mut Vec<u8>);
}

impl WriteLEBytes for u32 {
    fn write_bytes(&self, buffer: &mut Vec<u8>) {
        buffer.extend_from_slice(&self.to_le_bytes());
    }
}

impl WriteLEBytes for u64 {
    fn write_bytes(&self, buffer: &mut Vec<u8>) {
        buffer.extend_from_slice(&self.to_le_bytes());
    }
}

impl WriteLEBytes for i32 {
    fn write_bytes(&self, buffer: &mut Vec<u8>) {
        buffer.extend_from_slice(&self.to_le_bytes());
    }
}

impl WriteLEBytes for String {
    fn write_bytes(&self, buffer: &mut Vec<u8>) {
        let len = self.len() as u64;
        buffer.extend_from_slice(&len.to_le_bytes());

        buffer.extend_from_slice(self.as_bytes());
    }
}

impl WriteLEBytes for Value {
    fn write_bytes(&self, buffer: &mut Vec<u8>) {
        match self {
            Value::Int(num) => num.write_bytes(buffer),
            Value::UInt(num) => num.write_bytes(buffer),
            Value::RowID(RowID(num)) => num.write_bytes(buffer),
            Value::Varchar(st) => st.write_bytes(buffer),
        }
    }
}


// Read from file helpers
fn read_u32<F: Read>(file: &mut F) -> Result<u32, DatabaseError> {
    let mut buffer = [0u8; 4];
    file.read_exact(&mut buffer).map_err(|e| DatabaseError::IOError(e))?;

    let num = u32::from_le_bytes(buffer);

    Ok(num)
}
fn read_i32<F: Read>(file: &mut F) -> Result<i32, DatabaseError> {
    let mut buffer = [0u8; 4];
    file.read_exact(&mut buffer).map_err(|e| DatabaseError::IOError(e))?;

    let num = i32::from_le_bytes(buffer);

    Ok(num)
}

fn read_u64<F: Read>(file: &mut F) -> Result<u64, DatabaseError> {
    let mut buffer = [0u8; 8];
    file.read_exact(&mut buffer).map_err(|e| DatabaseError::IOError(e))?;

    let num = u64::from_le_bytes(buffer);

    Ok(num)
}

fn read_string<F: Read>(file: &mut F) -> Result<String, DatabaseError> {
    let length = read_u64(file)?;

    let mut buffer = vec![0u8; length as usize];

    file.read_exact(&mut buffer).map_err(|e| DatabaseError::IOError(e))?;

    String::from_utf8(buffer).map_err(|_| DatabaseError::InvalidUTF8)

}

// helper to get typeid enum
const GET_TYPE_ID: [TypeID; 4] = [TypeID::Int, TypeID::UInt, TypeID::RowID, TypeID::Varchar];

//parses one column from a file
fn parse_column<F: Seek + Read>(file: &mut F, rows: u64, column_info: (TypeID, usize)) -> Result<Vec<Value>, DatabaseError> {
    let typeid = column_info.0;
    let index_start = column_info.1;

    file.seek(SeekFrom::Start(index_start as u64)).map_err(|e| DatabaseError::IOError(e))?;
    let mut vec: Vec<Value> = Vec::new();

    for _ in 0..rows {
        match typeid {
            TypeID::Int => {
                let value = read_i32(file)?;
                vec.push(Value::Int(value));
            },
            TypeID::UInt => {
                let value = read_u32(file)?;
                vec.push(Value::UInt(value));
            },
            TypeID::RowID => {
                let value = read_u64(file)?;
                vec.push(Value::RowID(RowID(value)));
            },
            TypeID::Varchar => {
                let st = read_string(file)?;
                vec.push(Value::Varchar(Rc::from(st)));
            }
        }
    }

    Ok(vec)
}

impl DataFile {
    pub fn new(header: DataFileHeader, data: TableChunk) -> Self {
        DataFile { header, data }
    }

    /// Serializes [`DataFile`] as specified in lab slides.
    pub fn to_bytes(&self) -> Vec<u8> {
        let header_len = self.header.byte_length();
        let mut vec = Vec::new();

        let mut indexes: Vec<usize> = Vec::new();

        let mut index = header_len;

        for (column, typeid) in self.data.iter().zip(self.header.column_info.iter()) {
            indexes.push(index); // add every start index to the index list

            match typeid { // calculate next index
                TypeID::RowID => index += 8 * column.len(),
                TypeID::Int => index += 4 * column.len(),
                TypeID::UInt => index += 4 * column.len(),
                TypeID::Varchar => {} // cannot be calculated
            }

            for elem in column.iter() {
                elem.write_bytes(&mut vec); // write every value to the buffer
                if *typeid == TypeID::Varchar {
                    let Value::Varchar(st) = elem else { panic!() };
                    index += 8 + st.len(); // calculate index for strings by adding every string's length
                }
            }

        }

        let mut header_vec = self.header.to_bytes(&indexes); // get the header

        header_vec.reserve(vec.len());
        header_vec.append(&mut vec); // add the data to the header

        header_vec
    }

    /// Deserialize all columns into [`DataFile`] as specified in lab slides.
    pub fn parse<F: Seek + Read>(file: &mut F) -> Result<Self, DatabaseError> {
        let read_header = ReadDataFileHeader::parse(file).expect("header_error");
        let mut data = TableChunk::new();

        for column in read_header.column_info.iter() {
            data.push(parse_column(file, read_header.rows, *column).expect("parse_column_error")); // parse each column
        }

        let datafile = DataFile {
            header: read_header.into(),
            data
        };
        Ok(datafile)
    }

    /// Deserialize selected columns into [`DataFile`] as specified in lab slides.
    /// Especially ensure that TableChunk.len() == ReadFileHeader.columns.
    pub fn parse_columns<F: Seek + Read>(
        file: &mut F,
        column_indexes: Columns,
    ) -> Result<Self, DatabaseError> {
        match column_indexes {
            Columns::All => Self::parse(file),
            Columns::Selection(sel) => {
                let read_header = ReadDataFileHeader::parse(file).expect("header error");
                let mut data = TableChunk::new();

                for column in 0..read_header.columns {
                    if sel.contains(&(column as usize)) { // parse selected columns
                        data.push(parse_column(file, read_header.rows, read_header.column_info[column as usize]).expect("parse_column(s) error"));
                    } else {
                        data.push(Vec::new()); // add empty vec for unselected columns
                    }
                }

                let datafile = DataFile {
                    header: read_header.into(),
                    data
                };
                Ok(datafile)
            }
        }
    }
}

impl DataFileHeader {
    const MAGIC: [u8; 8] = [0x53, 0x44, 0x4d, 0x53, 0x19, 0x03, 0x4a, 0x53];
    const HDR_LEN: usize = Self::MAGIC.len() + 16; // 8 byte for rows, 8 bytes for columns
    const COLUMN_INFO_LEN: usize = 16;

    pub fn new(rows: u64, columns: u64, column_info: Vec<TypeID>) -> Self {
        DataFileHeader {
            rows,
            columns,
            column_info,
        }
    }

    /// Serialize the header into a Vec<u8>.
    ///
    /// # Arguments
    ///
    /// * `column_start_indexes`: The column start indexes to write into the serialized header.
    ///
    /// returns: Vec<u8, Global>
    fn to_bytes(&self, column_start_indexes: &[usize]) -> Vec<u8> {
        let mut vec = Vec::new();
        vec.extend_from_slice(&Self::MAGIC);
        self.rows.write_bytes(&mut vec);
        self.columns.write_bytes(&mut vec);

        for (typeid, start) in self.column_info.iter().zip(column_start_indexes.iter()) {
            (*typeid as u64).write_bytes(&mut vec);
            (*start as u64).write_bytes(&mut vec);
        }
        vec
    }

    /// Determine the byte length of the header. Especially useful in [DataFile::to_bytes].
    ///
    /// returns: usize
    fn byte_length(&self) -> usize {
        Self::HDR_LEN + self.columns as usize * Self::COLUMN_INFO_LEN
    }
}

impl ReadDataFileHeader {
    /// Parses the header of a data file including the column start indexes.
    ///
    /// # Arguments
    ///
    /// * `file`: The file to parse the header from.
    ///
    /// returns: Result<ReadDataFileHeader, DatabaseError>
    fn parse<F: Seek + Read>(file: &mut F) -> Result<Self, DatabaseError> {
        // checking magic byte
        let mut buffer= [0u8; 8];
        file.read_exact(&mut buffer).map_err(|e| DatabaseError::IOError(e)).expect("io_error"); // read magic
        if buffer != DataFileHeader::MAGIC { //check magic valid
            panic!()
        }
        
        let rows = read_u64(file).expect("");
        let columns = read_u64(file).expect("");
        let mut column_info = Vec::new();

        for _ in 0..columns {
            let typeid_usize = read_u64(file).expect("") as usize;
            let typeid: TypeID = GET_TYPE_ID.get(typeid_usize).ok_or(DatabaseError::Unknown).expect("").clone();
            let start_index = read_u64(file).expect("") as usize;

            column_info.push((typeid, start_index)); // read column info
        }

        let header = ReadDataFileHeader {
            rows,
            columns,
            column_info
        };
        Ok(header)
    }
}
