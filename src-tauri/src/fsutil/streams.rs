use std::io;
use std::os::windows::io::AsRawHandle;

use windows::Win32::Foundation::{
    ERROR_HANDLE_EOF, ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER,
    ERROR_MORE_DATA, ERROR_NOT_SUPPORTED, HANDLE,
};
use windows::Win32::Storage::FileSystem::{
    FileStreamInfo, GetFileInformationByHandleEx, GetVolumeInformationByHandleW, FILE_STREAM_INFO,
};
const FILE_NAMED_STREAMS: u32 = 0x0004_0000;

pub(crate) fn reject_named_data_streams(file: &std::fs::File) -> io::Result<()> {
    check(HANDLE(file.as_raw_handle()))
}

pub(super) fn check(handle: HANDLE) -> io::Result<()> {
    const NAME_OFFSET: usize = std::mem::offset_of!(FILE_STREAM_INFO, StreamName);
    // UTF-16 stream name for the unnamed data stream.
    const DEFAULT_DATA: [u16; 7] = [58, 58, 36, 68, 65, 84, 65];
    // The API requires an eight-byte-aligned buffer. A full buffer cannot
    // describe just the unnamed data stream.
    let mut buffer = [0u64; 128];
    let result = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileStreamInfo,
            buffer.as_mut_ptr().cast(),
            std::mem::size_of_val(&buffer) as u32,
        )
    };
    match result {
        Ok(()) => {
            let header = unsafe { &*buffer.as_ptr().cast::<FILE_STREAM_INFO>() };
            if header.NextEntryOffset == 0 {
                let name_bytes = header.StreamNameLength as usize;
                if name_bytes == 0 {
                    return Ok(());
                }
                if name_bytes == std::mem::size_of_val(&DEFAULT_DATA) {
                    let name = unsafe {
                        std::slice::from_raw_parts(
                            buffer.as_ptr().cast::<u8>().add(NAME_OFFSET).cast::<u16>(),
                            DEFAULT_DATA.len(),
                        )
                    };
                    if name == DEFAULT_DATA {
                        return Ok(());
                    }
                }
            }
        }
        Err(error) if error.code() == ERROR_HANDLE_EOF.to_hresult() => return Ok(()),
        Err(error)
            if [
                ERROR_INVALID_FUNCTION,
                ERROR_INVALID_PARAMETER,
                ERROR_NOT_SUPPORTED,
            ]
            .iter()
            .any(|code| code.to_hresult() == error.code()) =>
        {
            let mut flags = 0;
            unsafe {
                GetVolumeInformationByHandleW(handle, None, None, None, Some(&mut flags), None)
            }
            .map_err(io::Error::other)?;
            if flags & FILE_NAMED_STREAMS == 0 {
                return Ok(());
            }
            return Err(io::Error::other(error));
        }
        Err(error)
            if [ERROR_MORE_DATA, ERROR_INSUFFICIENT_BUFFER]
                .iter()
                .any(|code| code.to_hresult() == error.code()) => {}
        Err(error) => return Err(io::Error::other(error)),
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "named data streams cannot be backed up or restored",
    ))
}
