use std::sync::Mutex;

/// One `USER_PROCESS` entry of the login accounting database.
#[derive(Clone, Debug)]
pub struct LoginSession {
    pub user: String,
    pub line: String,
    pub host: String,
    pub pid: i32,
    pub seconds: i64,
    pub micros: u32,
}

/// The utmpx cursor is process-global state.
static CURSOR: Mutex<()> = Mutex::new(());

pub fn sessions() -> Vec<LoginSession> {
    let _guard = CURSOR.lock().unwrap_or_else(|e| e.into_inner());
    let mut out = vec![];
    unsafe { libc::setutxent() };
    loop {
        let entry = unsafe { libc::getutxent() };
        if entry.is_null() {
            break;
        }
        // SAFETY: getutxent returns a pointer to static storage that stays valid until the next call.
        let entry = unsafe { &*entry };
        if entry.ut_type == libc::USER_PROCESS {
            out.push(LoginSession {
                user: super::c_chars(&entry.ut_user),
                line: super::c_chars(&entry.ut_line),
                host: super::c_chars(&entry.ut_host),
                pid: entry.ut_pid,
                seconds: entry.ut_tv.tv_sec,
                micros: entry.ut_tv.tv_usec.max(0) as u32,
            });
        }
    }
    unsafe { libc::endutxent() };
    out
}

/// Number of distinct user names with a login session.
pub fn distinct_user_count() -> usize {
    let mut names: Vec<String> = sessions().into_iter().map(|s| s.user).collect();
    names.sort();
    names.dedup();
    names.len()
}
