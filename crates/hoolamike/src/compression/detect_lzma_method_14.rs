use std::{
    fs::File,
    io::{self, BufReader, Read, Seek, SeekFrom},
};

pub(crate) fn is_zip_lzma_method_14(file: &File) -> io::Result<bool> {
    let mut reader = BufReader::new(file);

    loop {
        // Read the first 30 bytes (local file header base)
        let mut header = [0u8; 30];
        if let Err(e) = reader.read_exact(&mut header) {
            // If we can't read 30 bytes, we're likely at EOF or a malformed header.
            // We'll just stop and return false if no file was found to use LZMA.
            if e.kind() == io::ErrorKind::UnexpectedEof {
                return Ok(false);
            } else {
                return Err(e);
            }
        }

        // 1. Check local file header signature
        let signature = u32::from_le_bytes(header[0..4].try_into().unwrap());
        if signature != 0x04034b50 {
            // Not a valid local file header => break or return false
            // (Could also skip looking for next signature in a more advanced parser)
            return Ok(false);
        }

        // 2. Get the compression method (2 bytes at offset 8)
        let compression_method = u16::from_le_bytes(header[8..10].try_into().unwrap());
        if compression_method == 14 {
            return Ok(true); // Found a file using LZMA
        }

        // 3. Calculate how many bytes to skip:
        let file_name_length = u16::from_le_bytes(header[26..28].try_into().unwrap()) as u64;
        let extra_field_length = u16::from_le_bytes(header[28..30].try_into().unwrap()) as u64;
        let compressed_size = u32::from_le_bytes(header[18..22].try_into().unwrap()) as u64;

        // Skip the filename, extra field, and compressed data
        // (the local file header + data block is done; move to next header)
        let skip_amount = file_name_length + extra_field_length + compressed_size;
        reader.seek(SeekFrom::Current(skip_amount as i64))?;
    }
}
