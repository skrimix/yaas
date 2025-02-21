use std::error::Error;

use sysproxy::Sysproxy;
use tracing::{debug, error};

mod speed;
pub use speed::*;

// #[instrument(ret, level = "trace")]
// TODO: use this for downloads
pub fn get_sys_proxy() -> Option<String> {
    let proxy = Sysproxy::get_system_proxy();
    match proxy {
        Ok(proxy) => {
            if proxy.enable {
                let result = format!("http://{}:{}", proxy.host, proxy.port);
                debug!(proxy = &result, "got system proxy");
                return Some(result);
            }
        }
        Err(e) => {
            error!(error = &e as &dyn Error, "failed to get system proxy");
        }
    }
    None
}
