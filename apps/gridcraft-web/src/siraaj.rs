//! Siraaj host bridge (`?siraaj`): the embedding page opens a workbook, drives the editor with
//! control requests (`docs/control-protocol.md`) and receives saved bytes, over `postMessage`.
//! It replaces the desktop's TCP control port, which a browser cannot open.
//!
//! Parent → editor (`event.source` must be `window.parent`):
//! - `{type: "siraaj.open", name, bytes: ArrayBuffer}`: open a workbook.
//! - `{type: "siraaj.save"}`: reply with `siraaj.saved` carrying the active workbook as `.xlsx`.
//! - `{type: "siraaj.control", id, method, params}`: one control request.
//!
//! Editor → parent: `siraaj.ready`, `siraaj.reply {id, reply}`, `siraaj.saved {name, bytes}`,
//! `siraaj.dirty {dirty}`, `siraaj.selection {sheet, ranges}` (sent when the selection changes).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender};

use gridcraft_ui_egui::{ControlRequest, Inbox, SheetApp};
use serde_json::{Value, json};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;

type Pending = Rc<RefCell<Vec<(JsValue, Receiver<Value>)>>>;

pub struct Siraaj {
    pending: Pending,
    save_requested: Rc<Cell<bool>>,
    dirty: Cell<Option<bool>>,
    selection: RefCell<String>,
}

fn post(msg: &JsValue, transfer: Option<&JsValue>) {
    let Some(window) = web_sys::window() else { return };
    let Some(parent) = window.parent().ok().flatten() else { return };
    // The host page is served from the same origin as the editor; never broadcast to others.
    let Ok(origin) = window.location().origin() else { return };
    let _ = match transfer {
        Some(t) => parent.post_message_with_transfer(msg, &origin, &js_sys::Array::of1(t)),
        None => parent.post_message(msg, &origin),
    };
}

fn message(ty: &str, fields: &[(&str, JsValue)]) -> JsValue {
    let o = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&o, &"type".into(), &ty.into());
    for (k, v) in fields {
        let _ = js_sys::Reflect::set(&o, &(*k).into(), v);
    }
    o.into()
}

fn json_to_js(v: &Value) -> JsValue {
    js_sys::JSON::parse(&v.to_string()).unwrap_or(JsValue::NULL)
}

fn js_to_json(v: &JsValue) -> Value {
    js_sys::JSON::stringify(v).ok().and_then(|s| s.as_string()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null)
}

fn field(o: &JsValue, k: &str) -> JsValue {
    js_sys::Reflect::get(o, &k.into()).unwrap_or(JsValue::UNDEFINED)
}

impl Siraaj {
    /// Starts the listener and gives the control receiver to `app`.
    pub fn start(app: &mut SheetApp, inbox: Inbox, ctx: egui::Context) -> Self {
        let (tx, rx): (Sender<ControlRequest>, Receiver<ControlRequest>) = std::sync::mpsc::channel();
        app.control_rx = Some(rx);
        let pending: Pending = Rc::default();
        let save_requested: Rc<Cell<bool>> = Rc::default();
        let (pending_l, save_l) = (pending.clone(), save_requested.clone());
        let on_message = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |e: web_sys::MessageEvent| {
            let parent = web_sys::window().and_then(|w| w.parent().ok().flatten());
            if !matches!((e.source(), parent), (Some(s), Some(p)) if js_sys::Object::is(&s, &p)) {
                return;
            }
            let data = e.data();
            match field(&data, "type").as_string().as_deref() {
                Some("siraaj.open") => {
                    let name = field(&data, "name").as_string().unwrap_or_else(|| "workbook.xlsx".into());
                    let bytes = js_sys::Uint8Array::new(&field(&data, "bytes")).to_vec();
                    inbox.lock().unwrap_or_else(|e| e.into_inner()).push((name, bytes));
                }
                Some("siraaj.save") => save_l.set(true),
                Some("siraaj.control") => {
                    let method = field(&data, "method").as_string().unwrap_or_default();
                    let params = Some(js_to_json(&field(&data, "params"))).filter(|v| !v.is_null()).unwrap_or(json!({}));
                    let (req, reply_rx) = ControlRequest::new(method, params);
                    if tx.send(req).is_ok() {
                        pending_l.borrow_mut().push((field(&data, "id"), reply_rx));
                    }
                }
                _ => {}
            }
            ctx.request_repaint();
        });
        if let Some(w) = web_sys::window() {
            let _ = w.add_event_listener_with_callback("message", on_message.as_ref().unchecked_ref());
        }
        on_message.forget();
        post(&message("siraaj.ready", &[]), None);
        Self { pending, save_requested, dirty: Cell::new(None), selection: RefCell::default() }
    }

    /// After each frame: answer finished control requests, save on request, report changes.
    pub fn sync(&self, app: &mut SheetApp) {
        self.pending.borrow_mut().retain(|(id, rx)| match rx.try_recv() {
            Ok(v) => {
                post(&message("siraaj.reply", &[("id", id.clone()), ("reply", json_to_js(&v))]), None);
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(_) => false,
        });
        if self.save_requested.replace(false)
            && let Ok(r) = app.run("file.saveBytes", json!({"format": "xlsx"}))
            && let Some(bytes) = r.get("base64").and_then(Value::as_str).and_then(gridcraft_engine::io::base64_decode)
        {
            let name = r.get("name").and_then(Value::as_str).unwrap_or("Book.xlsx");
            let buf = js_sys::Uint8Array::from(bytes.as_slice()).buffer();
            post(&message("siraaj.saved", &[("name", name.into()), ("bytes", buf.clone().into())]), Some(&buf.into()));
            if let Some(doc) = app.session.active_mut() {
                doc.saved = doc.wb.clone();
            }
        }
        let dirty = app.session.documents().iter().any(|d| d.is_dirty());
        if self.dirty.get() != Some(dirty) {
            self.dirty.set(Some(dirty));
            post(&message("siraaj.dirty", &[("dirty", dirty.into())]), None);
        }
        if let Some(doc) = app.session.active() {
            let sheet = doc.wb.active().map(|s| s.name.clone()).unwrap_or_default();
            let ranges: Vec<String> = doc.selection.ranges.iter().map(ToString::to_string).collect();
            let key = format!("{sheet}!{}", ranges.join(","));
            if *self.selection.borrow() != key {
                *self.selection.borrow_mut() = key;
                let list = js_sys::Array::new();
                for r in &ranges {
                    list.push(&JsValue::from_str(r));
                }
                post(&message("siraaj.selection", &[("sheet", sheet.as_str().into()), ("ranges", list.into())]), None);
            }
        }
    }
}
