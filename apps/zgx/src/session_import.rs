//! The session picker uses the same import controller as the mobile clients.
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
};

use zeron_client::session_import::{ImportController, ImportHost};
use zeron_proto::EngineInfo;
use zgx_script::{ScriptHost, Shared, Value};

use crate::{engine, plugins::preview, sessions};

struct Picker {
    local: EngineInfo,
    hosts: Arc<Mutex<Vec<ImportHost>>>,
    controller: Arc<ImportController>,
}

fn publish(snapshot: &Shared, controller: &ImportController) {
    let json = serde_json::to_string(&controller.snapshot()).expect("import snapshot serializes");
    snapshot.set(Value::from_json(&json).expect("import snapshot is JSON"));
}

pub fn install(host: &ScriptHost) {
    let picker = Rc::new(RefCell::new(None::<Picker>));
    let snapshot = host.shared("session_import.snapshot", Value::Nil);
    host.shared(
        "session_import.providers",
        Value::List(
            zeron_proto::SESSION_IMPORT_PROVIDERS
                .iter()
                .map(|(_, id, label)| {
                    Value::record([("value", Value::from(*id)), ("label", Value::from(*label))])
                })
                .collect::<Vec<_>>()
                .into(),
        ),
    );

    let state = picker.clone();
    let shared = snapshot.clone();
    host.async_action("session_import.open", move |_| {
        let (picker, snapshot) = (state.clone(), shared.clone());
        let allowed = preview::refuse("session_import.open");
        async move {
            allowed?;
            let info = engine::get()
                .call("EngineInfo", serde_json::json!({}))
                .await?;
            let local: EngineInfo =
                serde_json::from_value(info).map_err(|error| error.to_string())?;
            if picker.borrow().is_some() {
                return Ok(Value::Nil);
            }
            let (port, dir) = engine::get().ipc_endpoint().ok_or("offline")?;
            let hosts = Arc::new(Mutex::new(sessions::import_hosts(&local)));
            let list = hosts.clone();
            let controller =
                ImportController::over_ipc(port, dir, move || list.lock().unwrap().clone());
            let mut watch = controller.subscribe();
            publish(&snapshot, &controller);
            let weak = Arc::downgrade(&controller);
            *picker.borrow_mut() = Some(Picker {
                local,
                hosts,
                controller,
            });
            engine::get().spawn(async move {
                while watch.changed().await.is_ok() {
                    let Some(controller) = weak.upgrade() else {
                        break;
                    };
                    publish(&snapshot, &controller);
                }
            });
            Ok(Value::Nil)
        }
    });

    let state = picker.clone();
    host.action("session_import.update", move |args| {
        let picker = state.borrow();
        let Some(Picker {
            local,
            hosts,
            controller,
        }) = picker.as_ref()
        else {
            return;
        };
        let params = crate::plugins::value_json(&args);
        let value = params["value"].as_str();
        let text = value.unwrap_or_default();
        match params["action"].as_str() {
            Some("hosts") => {
                *hosts.lock().unwrap() = sessions::import_hosts(local);
                controller.update_hosts();
            }
            Some("device") => controller.select_device(text),
            Some("query") => controller.set_query(text.into()),
            Some("provider") => controller.set_provider(value.map(str::to_owned)),
            Some("toggle") => controller.toggle(text),
            Some("select_visible") => controller.select_visible(),
            Some("refresh") => controller.refresh(),
            Some("more") => controller.load_more(),
            Some("start") => controller.start(false),
            Some("retry") => controller.start(true),
            Some("cancel") => controller.cancel(),
            _ => {}
        }
    });
    host.action("session_import.close", move |_| {
        if let Some(picker) = picker.borrow_mut().take() {
            picker.controller.close();
        }
        snapshot.set(Value::Nil);
    });
}
