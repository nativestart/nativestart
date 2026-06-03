use binrw::io::NoSeek;
use binrw::{binrw, BinRead, BinResult, BinWrite};
use flate2::Compression;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::io;
use flate2::write::DeflateEncoder;

const LOCAL_FILE_HEADER_SIGNATURE: u32 = 0x0403_4b50;
const CENTRAL_DIRECTORY_HEADER_SIGNATURE: u32 = 0x0201_4b50;
const CENTRAL_DIRECTORY_END_SIGNATURE: u32 = 0x0605_4b50;

#[binrw]
#[brw(little)]
struct Signature(u32);

#[binrw]
#[brw(little)]
struct LocalHeader {
    pub version_made_by: u16,
    pub flags: u16,
    pub compression_method: u16,
    pub last_mod_time: u16,
    pub last_mod_date: u16,
    pub crc32: u32,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    pub file_name_length: u16,
    pub extra_field_length: u16,
}

#[binrw]
#[brw(little)]
struct DataDescriptor {
    pub crc32: u32,
    pub compressed_size: u32,
    pub uncompressed_size: u32
}

#[binrw]
#[brw(little)]
struct CentralDirectoryEntry {
    pub version_made_by: u16,
    pub version_to_extract: u16,
    pub flags: u16,
    pub compression_method: u16,
    pub last_mod_time: u16,
    pub last_mod_date: u16,
    pub crc32: u32,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    pub file_name_length: u16,
    pub extra_field_length: u16,
    pub file_comment_length: u16,
    pub disk_number: u16,
    pub internal_file_attributes: u16,
    pub external_file_attributes: u32,
    pub offset: u32,
}

#[binrw]
#[brw(little)]
struct CentralDirectoryEnd {
    pub disk_number: u16,
    pub disk_with_central_directory: u16,
    pub number_of_files_on_this_disk: u16,
    pub number_of_files: u16,
    pub central_directory_size: u32,
    pub central_directory_offset: u32,
}

pub fn recompress<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> BinResult<()> {
    let mut input = NoSeek::new(reader);
    let mut output = NoSeek::new(writer);

    let mut input_position:u32 = 0;
    let mut output_position:u32 = 0;
    let mut offsets = HashMap::new();
    let mut compressed_size = HashMap::new();
    loop {
        let magic = Signature::read(&mut input)?;
        magic.write(&mut output)?;
        if magic.0 == LOCAL_FILE_HEADER_SIGNATURE {
            let mut entry = LocalHeader::read(&mut input)?;
            let data_descriptor = entry.flags & (1 << 3) == 1;

            let mut header_size = 30 + (entry.file_name_length + entry.extra_field_length) as u32;
            let old_data_size = entry.compressed_size;

            let new_data_size = if entry.compression_method == 0 && entry.uncompressed_size > 0 {
                let mut file_name = vec![0u8; entry.file_name_length as usize];
                input.read_exact(&mut file_name)?;

                let mut extra_field = vec![0u8; entry.extra_field_length as usize];
                input.read_exact(&mut extra_field)?;

                let mut data_uncompressed = vec![0u8; entry.uncompressed_size as usize];
                input.read_exact(&mut data_uncompressed)?;

                let mut e = DeflateEncoder::new(Vec::new(), Compression::default());
                e.write_all(&data_uncompressed)?;
                let compressed = e.finish()?;

                let new_size = compressed.len() as u32;
                entry.compression_method = 8;
                entry.compressed_size = new_size;
                entry.write(&mut output)?;
                output.write_all(&file_name)?;
                output.write_all(&extra_field)?;
                output.write_all(&compressed)?;

                if data_descriptor {
                    let mut desc = DataDescriptor::read(&mut input)?;
                    desc.compressed_size = new_size;
                    desc.write(&mut output)?;
                    header_size += 12;
                }
                new_size
            } else {
                entry.write(&mut output)?;
                copy(&mut input, &mut output, (entry.file_name_length + entry.extra_field_length) as u32
                    + entry.compressed_size + (if data_descriptor {12} else {0}));
                old_data_size
            };

            compressed_size.insert(input_position, new_data_size);
            offsets.insert(input_position, output_position);
            input_position += header_size + old_data_size;
            output_position += header_size + new_data_size;
        } else if magic.0 == CENTRAL_DIRECTORY_HEADER_SIGNATURE {
            let mut entry = CentralDirectoryEntry::read(&mut input)?;
            let old_value = entry.offset;
            entry.compression_method = if entry.uncompressed_size > 0 {8} else {0};
            entry.compressed_size = *compressed_size.get(&old_value).unwrap();
            entry.offset = *offsets.get(&old_value).unwrap();
            entry.write(&mut output)?;
            copy(&mut input, &mut output, (entry.file_name_length + entry.extra_field_length + entry.file_comment_length) as u32)
        } else if magic.0 == CENTRAL_DIRECTORY_END_SIGNATURE {
            let mut entry = CentralDirectoryEnd::read(&mut input)?;
            entry.central_directory_offset = output_position;
            entry.write(&mut output)?;
            io::copy(&mut input, &mut output)?;
            break
        } else {
            panic!("Unsupported magic {:x}", magic.0);
        }
    }

    Ok(())
}

fn copy<R: Read, W: Write>(reader: &mut R, writer: &mut W, bytes: u32) {
    let mut buffer = vec![0u8; bytes as usize];
    reader.read_exact(&mut buffer).unwrap();
    writer.write_all(&buffer).unwrap();
}
