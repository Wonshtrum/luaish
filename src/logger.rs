use crate::source::Source;
use std::sync::Mutex;

pub static LOGGER: Mutex<Source> = Mutex::new(Source::empty(String::new()));

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {{
        let now = ::time::OffsetDateTime::now_utc();
        let mut logger = $crate::logger::LOGGER.lock().unwrap();
        logger.append(&format!("[{:02}:{:02}:{:02}] {}:{}: ",
            now.hour(),
            now.minute(),
            now.second(),
            module_path!(), line!(),
        ));
        logger.append(&format!(
            $($arg)*
        ));
        logger.append("\n");
    }};
}
