//! Reproduce `td::TdDb::as_db_key` (TdDb.cpp:671-676) and
//! `td::TdDb::check_parameters` (TdDb.cpp:621-669) on Windows.
//!
//! The framing (script grammar, narration) is the one already fixed by the
//! committed `rust/src/bin/binlog_write_probe.rs` + `tools/fixtures/binlog_write_probe.cpp`
//! pair: a tab/space separated script in, one narration line per command out,
//! compared byte for byte against the oracle by tools/tddb_paths_diff.py.
//!
//! What is ported, and from where:
//!   * `DbKey` (tddb/td/db/DbKey.h) -- three observable facts, empty/raw_key/
//!     password and the `data()` bytes; `raw_key` never produces an empty key,
//!     so `as_db_key("")` is the raw key `cucumber`.
//!   * `TdDb::check_parameters`: the `"."` default, the implication chain
//!     msg -> chat -> file (it only ever *sets* flags), `prepare_dir` (append
//!     TD_DIR_SLASH, `mkpath(…, 0750)`, `realpath(…, true)`, append again if the
//!     resolved path lost it) and the two 400 messages.
//!   * `mkpath` / `mkdir` / `realpath` (tdutils/td/utils/port/path.cpp:65-83 and
//!     the TD_PORT_WINDOWS block :409-466), because the composed behaviour is
//!     what the criterion is about. Windows semantics come from the same Win32
//!     calls td uses: `CreateDirectoryW` (td reaches it through the app-container
//!     shim `CreateDirectoryFromAppW`, FromApp.h:64, which resolves to plain
//!     `CreateDirectory`) and `GetFullPathNameW`.
//!   * `winerror_to_string` (tdutils/td/utils/Status.cpp:42-57) and the two
//!     `Status` renderings that leak into a message: `[Error : code : message]`
//!     and `[WindowsError : <FormatMessage> : code : message]`, plus
//!     `move_as_error_suffix`, which `mkpath` uses to chain the first failing
//!     component behind the last one.
//!
//! Measured traps this port must keep (see docs/facts.md F193/F194):
//!   * `TD_DIR_SLASH` is `'\\'`, so `mkpath` walks *backslashes* only: a path
//!     written with '/' separators yields exactly one `CreateDirectoryW` call
//!     for the whole path (minus the separator `prepare_dir` appended), so its
//!     missing parents are never created.
//!   * `last_error` is assigned on *every* `mkdir` call, so a run of failures
//!     followed by a success is not an error; when the last error differs from
//!     the first one, `mkpath` returns `last_error + ": " + format(first_error)`.
//!   * `mkdir` treats `ERROR_ALREADY_EXISTS` as success and Windows reports that
//!     same code for a path that is an existing regular file, so a file passes
//!     `check_parameters` as a "directory".
//!   * `realpath` here is `GetFullPathNameW`: lexical, collapses `.`/`..`, folds
//!     '/' to '\\', preserves one trailing separator (`C:\a\` stays, `C:\a` gains
//!     nothing) and collapses a doubled one -- it never touches the disk, so a
//!     nonexistent path resolves happily and `realpath(dir, true)` can fail only
//!     on a malformed argument (buffer/length), never on access. Because it
//!     preserves the separator `prepare_dir` appended, td's two
//!     `if (res.back() != TD_DIR_SLASH)` appends are no-ops on Windows; they are
//!     still ported, since that is a measurement and not a tautology.
//!     The `errno == ERROR_ACCESS_DENIED` branch in td is a source quirk (CRT
//!     errno compared to a Win32 code) and stays unreachable for this corpus.
//!
//! Build from `rust/`:
//!   cargo build --release --target x86_64-pc-windows-gnu --bin tddb_paths_probe

use std::ffi::c_void;
use std::process::ExitCode;

const TD_DIR_SLASH: char = '\\';
const MAX_PATH: usize = 260;
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_ALREADY_EXISTS: u32 = 183;
const FORMAT_MESSAGE_FROM_SYSTEM: u32 = 0x0000_1000;

#[allow(non_snake_case)]
extern "system" {
    fn GetLastError() -> u32;
    fn GetFullPathNameW(file_name: *const u16, buffer_length: u32, buffer: *mut u16, file_part: *mut *mut u16) -> u32;
    fn CreateDirectoryW(path_name: *const u16, security_attributes: *const c_void) -> i32;
    fn FormatMessageW(
        flags: u32,
        source: *const c_void,
        message_id: u32,
        language_id: u32,
        buffer: *mut u16,
        size: u32,
        arguments: *const *const c_void,
    ) -> u32;
}

// td compares CRT `errno` against ERROR_ACCESS_DENIED inside realpath's
// ignore_access_denied branch. That branch cannot trigger for this corpus
// (GetFullPathNameW fails only on malformed arguments), but the quirk is
// ported as written instead of dropped.
extern "C" {
    fn _errno() -> *mut i32;
}

fn crt_errno() -> i32 {
    unsafe { *_errno() }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StatusKind {
    General,
    Os,
}

#[derive(Clone)]
struct Status {
    kind: StatusKind,
    code: i32,
    message: String,
}

impl Status {
    fn error(code: i32, message: String) -> Status {
        Status { kind: StatusKind::General, code, message }
    }

    fn os_error(code: u32, message: String) -> Status {
        Status { kind: StatusKind::Os, code: code as i32, message }
    }

    fn os_error_now(message: String) -> Status {
        Status::os_error(unsafe { GetLastError() }, message)
    }

    fn message(&self) -> &str {
        &self.message
    }

    fn format(&self) -> String {
        match self.kind {
            StatusKind::General => format!("[Error : {} : {}]", self.code, self.message),
            StatusKind::Os => format!(
                "[WindowsError : {} : {} : {}]",
                winerror_to_string(self.code as u32),
                self.code,
                self.message
            ),
        }
    }

    fn with_suffix(self, suffix: &str) -> Status {
        Status { kind: self.kind, code: self.code, message: format!("{}{}", self.message, suffix) }
    }
}

fn winerror_to_string(code: u32) -> String {
    const SIZE: usize = 1000;
    let mut buffer = [0u16; SIZE];
    let size = unsafe {
        FormatMessageW(
            FORMAT_MESSAGE_FROM_SYSTEM,
            std::ptr::null(),
            code,
            0,
            buffer.as_mut_ptr(),
            (SIZE - 1) as u32,
            std::ptr::null(),
        )
    };
    if size == 0 {
        return "Unknown Windows error".to_string();
    }
    let mut length = size as usize;
    while length != 0 && (buffer[length - 1] == b'\n' as u16 || buffer[length - 1] == b'\r' as u16) {
        length -= 1;
    }
    match String::from_utf16(&buffer[..length]) {
        Ok(text) => text,
        Err(_) => "Invalid Windows error".to_string(),
    }
}

fn to_wstring(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

// from_wstring(wchar_t*, size_t): strict UTF-16 -> UTF-8, "Wrong wstring encoding".
fn from_wstring(units: &[u16]) -> Result<String, Status> {
    String::from_utf16(units).map_err(|_| Status::error(0, "Wrong wstring encoding".to_string()))
}

// Windows `mkdir` (path.cpp:409-423). Trailing separators are dropped first
// because CreateDirectoryW rejects them on a path that does not exist yet.
fn mkdir(dir: &str) -> Result<(), Status> {
    let mut wide = to_wstring(dir);
    while matches!(wide.last(), Some(&c) if c == b'/' as u16 || c == b'\\' as u16) {
        wide.pop();
    }
    wide.push(0);
    let  status = unsafe { CreateDirectoryW(wide.as_ptr(), std::ptr::null()) };
    if status == 0 {
        let error = unsafe { GetLastError() };
        if error != ERROR_ALREADY_EXISTS {
            return Err(Status::os_error(error, format!("Can't create directory \"{}\"", dir)));
        }
    }
    Ok(())
}

// mkpath (path.cpp:65-83). The separator is TD_DIR_SLASH only, `last_error` is
// overwritten by every call including the successful ones, and a chain of two
// distinct errors is reported as `last: first`.
fn mkpath(path: &str) -> Result<(), Status> {
    let bytes = path.as_bytes();
    let mut first_error: Option<Status> = None;
    let mut last_error: Option<Status> = None;
    for i in 1..bytes.len() {
        if bytes[i] == b'\\' {
            match mkdir(&path[..i]) {
                Ok(()) => last_error = None,
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error.clone());
                    }
                    last_error = Some(error);
                }
            }
        }
    }
    match last_error {
        None => Ok(()),
        Some(last) => {
            let first = first_error.expect("first_error is set together with last_error");
            if last.message() == first.message() && last.code == first.code {
                Err(first)
            } else {
                let suffix = format!(": {}", first.format());
                Err(last.with_suffix(&suffix))
            }
        }
    }
}

// realpath (path.cpp:433-466, Windows): GetFullPathNameW plus the trailing
// separator rule; the empty result is an error, not a fallback.
fn realpath(path: &str, ignore_access_denied: bool) -> Result<String, Status> {
    // td passes `wslice.c_str()`; a Rust Vec<u16> is not NUL terminated.
    let mut wide = to_wstring(path);
    wide.push(0);
    let mut buffer = [0u16; MAX_PATH + 1];
    let status = unsafe {
        GetFullPathNameW(wide.as_ptr(), MAX_PATH as u32, buffer.as_mut_ptr(), std::ptr::null_mut())
    };
    let mut res;
    if status == 0 {
        if ignore_access_denied && crt_errno() == ERROR_ACCESS_DENIED as i32 {
            res = path.to_string();
        } else {
            return Err(Status::os_error_now(format!("GetFullPathNameW failed for \"{}\"", path)));
        }
    } else {
        // from_wstring(const wchar_t*) stops at the NUL terminator.
        let length = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        res = from_wstring(&buffer[..length])?;
    }
    if res.is_empty() {
        return Err(Status::error(0, "Empty path".to_string()));
    }
    if !path.is_empty() && path.ends_with(TD_DIR_SLASH) && !res.ends_with(TD_DIR_SLASH) {
        res.push(TD_DIR_SLASH);
    }
    Ok(res)
}

fn prepare_dir(dir: &str) -> Result<String, Status> {
    assert!(!dir.is_empty(), "prepare_dir requires a non-empty directory");
    let mut dir = dir.to_string();
    if !dir.ends_with(TD_DIR_SLASH) {
        dir.push(TD_DIR_SLASH);
    }
    mkpath(&dir)?;
    let mut real_dir = realpath(&dir, true)?;
    if real_dir.is_empty() {
        return Err(Status::error(0, format!("Failed to get realpath for \"{}\"", dir)));
    }
    if !real_dir.ends_with(TD_DIR_SLASH) {
        real_dir.push(TD_DIR_SLASH);
    }
    Ok(real_dir)
}

struct Parameters {
    database_directory: String,
    files_directory: String,
    is_test_dc: bool,
    use_file_database: bool,
    use_chat_info_database: bool,
    use_message_database: bool,
}

// TdDb::check_parameters, including the fact that a failing database directory
// leaves every field untouched and a failing files directory keeps the *already
// normalised* database directory.
fn check_parameters(parameters: &mut Parameters) -> Result<(), Status> {
    if parameters.database_directory.is_empty() {
        parameters.database_directory = ".".to_string();
    }
    if parameters.use_message_database && !parameters.use_chat_info_database {
        parameters.use_chat_info_database = true;
    }
    if parameters.use_chat_info_database && !parameters.use_file_database {
        parameters.use_file_database = true;
    }

    let database_directory = prepare_dir(&parameters.database_directory).map_err(|error| {
        Status::error(
            400,
            format!(
                "Can't init database in the directory \"{}\": {}",
                parameters.database_directory,
                error.format()
            ),
        )
    })?;
    parameters.database_directory = database_directory;

    if parameters.files_directory.is_empty() {
        parameters.files_directory = parameters.database_directory.clone();
    } else {
        let files_directory = prepare_dir(&parameters.files_directory).map_err(|error| {
            Status::error(
                400,
                format!(
                    "Can't init files directory \"{}\": {}",
                    parameters.files_directory,
                    error.format()
                ),
            )
        })?;
        parameters.files_directory = files_directory;
    }
    Ok(())
}

#[allow(dead_code)]  // the full DbKey type is modelled; as_db_key only builds RawKey
enum DbKeyKind {
    Empty,
    RawKey,
    Password,
}

struct DbKey {
    kind: DbKeyKind,
    data: Vec<u8>,
}

impl DbKey {
    fn raw_key(data: Vec<u8>) -> DbKey {
        DbKey { kind: DbKeyKind::RawKey, data }
    }

    fn is_empty(&self) -> bool {
        matches!(self.kind, DbKeyKind::Empty)
    }

    fn is_raw_key(&self) -> bool {
        matches!(self.kind, DbKeyKind::RawKey)
    }

    fn is_password(&self) -> bool {
        matches!(self.kind, DbKeyKind::Password)
    }
}

fn as_db_key(key: Vec<u8>) -> DbKey {
    if key.is_empty() {
        return DbKey::raw_key(b"cucumber".to_vec());
    }
    DbKey::raw_key(key)
}

fn hex_encode(data: &[u8]) -> String {
    let mut res = String::with_capacity(data.len() * 2);
    for byte in data {
        res.push_str(&format!("{:02x}", byte));
    }
    res
}

fn hex_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if bytes.len() % 2 != 0 {
        return None;
    }
    let mut res = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks(2) {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        res.push(((hi << 4) | lo) as u8);
    }
    Some(res)
}

fn flag(token: &str) -> Option<bool> {
    match token {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

fn fail(line_no: usize, message: &str) -> ExitCode {
    eprintln!("tddb_paths_probe: {} in line {}", message, line_no);
    ExitCode::from(1)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: tddb_paths_probe <script.txt>");
        return ExitCode::from(2);
    }
    let text = match std::fs::read_to_string(&args[1]) {
        Ok(text) => text,
        Err(_) => {
            eprintln!("tddb_paths_probe: cannot read {}", args[1]);
            return ExitCode::from(2);
        }
    };

    for (index, line) in text.split('\n').enumerate() {
        let line_no = index + 1;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let split = match line.find([' ', '\t']) {
            Some(split) => split,
            None => return fail(line_no, "missing arguments"),
        };
        let command = &line[..split];
        let rest = &line[split + 1..];

        if command == "key" {
            let raw = if rest == "-" {
                Vec::new()
            } else {
                match hex_decode(rest) {
                    Some(raw) => raw,
                    None => return fail(line_no, "bad hex in key"),
                }
            };
            let in_hex = if raw.is_empty() { "-".to_string() } else { hex_encode(&raw) };
            let key = as_db_key(raw);
            println!(
                "key in={} kempty={} kraw={} kpwd={} dlen={} data={}",
                in_hex,
                key.is_empty() as u8,
                key.is_raw_key() as u8,
                key.is_password() as u8,
                key.data.len(),
                if key.data.is_empty() { "-".to_string() } else { hex_encode(&key.data) }
            );
            continue;
        }

        if command == "checkparams" {
            let fields: Vec<&str> = rest.split('\t').collect();
            if fields.len() != 6 {
                return fail(line_no, "checkparams needs 6 tab-separated fields");
            }
            let flags: Vec<Option<bool>> = fields[2..].iter().map(|field| flag(field)).collect();
            let (in_test_dc, in_file, in_chat, in_msg) = match (flags[0], flags[1], flags[2], flags[3]) {
                (Some(a), Some(b), Some(c), Some(d)) => (a, b, c, d),
                _ => return fail(line_no, "flags must be 0 or 1"),
            };
            let mut parameters = Parameters {
                database_directory: fields[0].to_string(),
                files_directory: fields[1].to_string(),
                is_test_dc: in_test_dc,
                use_file_database: in_file,
                use_chat_info_database: in_chat,
                use_message_database: in_msg,
            };
            let status = check_parameters(&mut parameters);
            let (ok, code, message) = match &status {
                Ok(()) => (1, 0, "OK".to_string()),
                Err(error) => (0, error.code, error.message().to_string()),
            };
            println!(
                "checkparams db_in=[{}] files_in=[{}] testdc={} file={} chat={} msg={} -> ok={} code={} \
                 db=[{}] files=[{}] testdc={} file={} chat={} msg={} msg=[{}]",
                fields[0],
                fields[1],
                in_test_dc as u8,
                in_file as u8,
                in_chat as u8,
                in_msg as u8,
                ok,
                code,
                parameters.database_directory,
                parameters.files_directory,
                parameters.is_test_dc as u8,
                parameters.use_file_database as u8,
                parameters.use_chat_info_database as u8,
                parameters.use_message_database as u8,
                message
            );
            continue;
        }

        return fail(line_no, "unknown command");
    }

    ExitCode::SUCCESS
}