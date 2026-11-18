// Server settings: Discord's guild administration. `api` builds each REST call, `access` decides which
// pages and controls the signed-in member may use, `audit` turns audit-log entries into sentences.
// GAP: the window itself lands once the shared settings shell (PR #18) is on main; until then nothing
// outside the tests calls these.
#![allow(dead_code)]

pub mod access;
pub mod api;
pub mod audit;
pub mod perms;
