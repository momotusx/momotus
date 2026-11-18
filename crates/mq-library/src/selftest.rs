//! The webOS storage checked under the TV's Nodes (`wasm-test.sh` drives it
//! with `tests/wasm/run.js` as the host, over a real directory). Each phase
//! runs in a fresh process, so `read` opens what an ended process left. A
//! failed check panics, and the hook reports it through `mqHost.died`.

use super::Library;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = mqHost)]
    fn died(message: &str);
}

fn pragma(lib: &Library, name: &str) -> String {
    lib.read(|c| {
        c.query_row(&format!("PRAGMA {name}"), [], |r| {
            r.get::<_, rusqlite::types::Value>(0)
        })
    })
    .map(|v| match v {
        rusqlite::types::Value::Integer(n) => n.to_string(),
        rusqlite::types::Value::Text(s) => s,
        other => format!("{other:?}"),
    })
    .unwrap()
}

/// `write`: a new database on the host VFS, a write read back at once on
/// the one connection, and a memory library beside it. `cut`: a write the
/// host kills before its commit ends (pages written, journal hot). `read`: the committed
/// value, the cut write rolled back, the file intact.
#[wasm_bindgen]
pub fn selftest(dir: &str, phase: &str) -> String {
    std::panic::set_hook(Box::new(|info| died(&info.to_string())));
    let path = format!("{dir}/library.sqlite");
    let lib = Library::open(&path).unwrap();
    let mut passed = Vec::new();
    match phase {
        "write" => {
            assert_eq!(pragma(&lib, "journal_mode"), "truncate");
            assert_eq!(pragma(&lib, "synchronous"), "2", "synchronous=FULL");
            assert!(lib.readers.is_empty(), "one connection on the host VFS");
            passed.push("host VFS in TRUNCATE and FULL, one connection");
            lib.set_device_value("k", "v").unwrap();
            assert_eq!(lib.device_value("k").unwrap().as_deref(), Some("v"));
            lib.set_device_value("k", "v").unwrap();
            assert_eq!(lib.device_value("k").unwrap().as_deref(), Some("v"));
            passed.push("write and read back to back");
            let memory = Library::in_memory().unwrap();
            memory.set_device_value("m", "1").unwrap();
            assert_eq!(memory.device_value("m").unwrap().as_deref(), Some("1"));
            let file: String = memory
                .read(|c| c.query_row("PRAGMA database_list", [], |r| r.get(2)))
                .unwrap();
            assert_eq!(file, "", "a memory library has no file");
            passed.push("memory library");
        }
        "cut" => {
            lib.set_device_value("k", "cut").unwrap();
            panic!("the host did not cut the write");
        }
        "read" => {
            assert_eq!(lib.device_value("k").unwrap().as_deref(), Some("v"));
            assert_eq!(pragma(&lib, "integrity_check"), "ok");
            passed.push("reopened, cut write rolled back, intact");
        }
        other => panic!("unknown phase {other}"),
    }
    passed.join(", ")
}
