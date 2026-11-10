//! Group G of docs/phase3-test-plan.md, how it runs (C212): what the panel's Context and Graphics choose is what Start
//! runs, a configuration keeps what travels and leaves what is the computer's, and the runner takes a configuration the
//! window saved. The models are GGUF files of a header that says what they need, made as long as their weights; the
//! cards are what a stand-in llama.cpp would list.

use std::path::{Path, PathBuf};

use eframe::egui;
use quickai_core::plan::{RunPlan, Start};

use super::graphics::{Devices, Fit, Place};
use super::run_settings::RunChoice;
use super::App;
use crate::config::{build_registry, ConfigDirs};
use crate::devices::{Device, DeviceKind};
use crate::settings::Settings;
use crate::testkit::scratch;

const GIB: u64 = 1024 * 1024 * 1024;

/// A GGUF file of a llama model: its header says `layers`, 8 heads of keys and values of 128 (so a token of context
/// takes `layers` x 8 x 256 x 2 bytes) and the context it was trained for, and the file is `bytes` long (sparse).
fn gguf(dir: &Path, name: &str, layers: u32, trained: u32, bytes: u64) -> PathBuf {
    fn text(b: &mut Vec<u8>, t: &str) {
        b.extend((t.len() as u64).to_le_bytes());
        b.extend(t.as_bytes());
    }
    let numbers: [(&str, u32); 5] = [
        ("llama.context_length", trained),
        ("llama.block_count", layers),
        ("llama.embedding_length", 4096),
        ("llama.attention.head_count", 32),
        ("llama.attention.head_count_kv", 8),
    ];
    let mut b = Vec::new();
    b.extend(0x4655_4747u32.to_le_bytes());
    b.extend(3u32.to_le_bytes());
    b.extend(0u64.to_le_bytes());
    b.extend((numbers.len() as u64 + 1).to_le_bytes());
    text(&mut b, "general.architecture");
    b.extend(8u32.to_le_bytes());
    text(&mut b, "llama");
    for (key, n) in numbers {
        text(&mut b, key);
        b.extend(4u32.to_le_bytes());
        b.extend(n.to_le_bytes());
    }
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    let file = std::fs::File::create(&path).unwrap();
    std::io::Write::write_all(&mut &file, &b).unwrap();
    file.set_len(bytes).unwrap();
    path
}

/// What a token of context takes for a model of `gguf`.
fn per_token(layers: u64) -> u64 {
    layers * 8 * 256 * 2
}

/// The application with llama.cpp (or `engine`), the models of `dir/models`, on a computer of `ram` bytes, the
/// configurations in `dir/config`.
fn app_in(dir: &Path, engine: &str, ram: u64) -> App {
    let mut settings = Settings::default();
    settings.set_chosen_engine(engine);
    settings.models_dir_override = Some(dir.join("models").display().to_string());
    let mut app = App::build_in(
        settings,
        build_registry(&ConfigDirs::default()),
        ram,
        Vec::new(),
        Some(dir.join("settings.json")),
        Some(dir.join("data").display().to_string()),
    );
    app.config_dirs = ConfigDirs {
        user: Some(dir.join("config")),
        data: Some(dir.join("data")),
        ..ConfigDirs::default()
    };
    app.bundled_models_dir = dir.join("bundled");
    app.binary = dir.join("llama-server");
    app.models = app.scan_models();
    app.selected = 0;
    app.reload_configurations();
    app.refresh_facts();
    app
}

/// A card as llama.cpp lists it.
fn card(id: &str, name: &str, total_gib: u64, free_gib: u64, kind: DeviceKind) -> Device {
    Device {
        id: id.to_string(),
        name: name.to_string(),
        total_bytes: total_gib * GIB,
        free_bytes: free_gib * GIB,
        kind,
    }
}

/// The installed llama.cpp lists `cards`, as `poll_devices` takes its answer: the cards are settled and the model
/// fitted to them.
fn with_cards(app: &mut App, cards: Vec<Device>, linked: bool) {
    app.devices = Devices::Known {
        binary: app.binary.clone(),
        list: cards,
        linked,
    };
    app.settle_devices();
    app.auto_fit();
}

/// The arguments Start would give llama-server now.
fn args(app: &App) -> Vec<String> {
    let plan: RunPlan = app.plan_to_show().expect("a plan");
    plan.units[0]
        .start
        .iter()
        .find_map(|s| match s {
            Start::Spawn(spawn) => Some(spawn.args.iter().map(|a| a.value.clone()).collect()),
            Start::Attach(_) => None,
        })
        .expect("a spawn")
}

fn after(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|at| args.get(at + 1).cloned())
}

/// G14: a length of the grid is the context Start runs with; 128K for a model trained for 128000 is 128000; Other…
/// keeps its own number; the longest the model takes is read from its file.
#[test]
fn p3_g14_a_length_of_the_grid_is_the_context_start_runs_with() {
    let dir = scratch("g14");
    gguf(&dir.join("models"), "Small-Q4_K_M.gguf", 16, 128_000, GIB);
    let mut app = app_in(&dir, "llamacpp", 32 * GIB);
    assert_eq!(
        app.model_facts().and_then(|f| f.context_length),
        Some(128_000)
    );
    assert_eq!(app.max_context(), 128_000);

    app.choose_length(65536);
    assert_eq!(after(&args(&app), "--ctx-size").as_deref(), Some("65536"));
    assert_eq!(app.run_choice, Some(RunChoice::Length(65536)));

    app.choose_length(131072);
    assert_eq!(after(&args(&app), "--ctx-size").as_deref(), Some("128000"));
    assert_eq!(
        app.choice_now(),
        RunChoice::Length(131072),
        "128000 is the box of 128K"
    );

    // a number of one's own
    app.values.set("ctx_size", 50_000i64);
    app.run_choice = Some(RunChoice::Own);
    assert_eq!(app.choice_now(), RunChoice::Own);
    assert_eq!(after(&args(&app), "--ctx-size").as_deref(), Some("50000"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// G15: the suggested length is the longest of 4K to 32K that fits on the card, or comfortably in the memory with no
/// card; each length's dot says where it fits.
#[test]
fn p3_g15_the_suggested_length_is_the_longest_that_fits_and_the_dots_say_where() {
    let dir = scratch("g15");
    // 4 GiB of weights, 32 layers: 32K of context is 4 GiB more
    gguf(&dir.join("models"), "Mid-Q4_K_M.gguf", 32, 131_072, 4 * GIB);
    let mut app = app_in(&dir, "llamacpp", 16 * GIB);
    assert_eq!(per_token(32) * 32768, 4 * GIB);

    // a 12 GiB card: 4 + 4 + the work fits in its 11 GiB beyond the margin
    with_cards(
        &mut app,
        vec![card("CUDA0", "RTX 4070 Ti", 12, 12, DeviceKind::Discrete)],
        false,
    );
    assert_eq!(app.suggested_context(), Some(32768));
    assert_eq!(app.fit(32768), Some(Fit::Good));
    assert_eq!(
        app.fit(131072),
        Some(Fit::Partly),
        "16 GiB of context is more than the card"
    );

    // an 8 GiB card: 16K is the longest that fits
    with_cards(
        &mut app,
        vec![card("CUDA0", "RTX 4060", 8, 8, DeviceKind::Discrete)],
        false,
    );
    assert_eq!(app.suggested_context(), Some(16384));

    // no card: comfortably in half of the 16 GiB
    with_cards(&mut app, Vec::new(), false);
    assert_eq!(
        app.place(),
        Place::Card,
        "no card listed leaves the layers as they were"
    );
    app.values.set("n_gpu_layers", 0i64);
    assert_eq!(app.suggested_context(), Some(16384));
    assert_eq!(
        app.fit(65536),
        Some(Fit::Partly),
        "12.5 GiB: tightly in four fifths of 16"
    );
    assert_eq!(app.fit(131072), Some(Fit::TooBig));
    let _ = std::fs::remove_dir_all(&dir);
}

/// G16: Auto puts the whole model on the card when it fits, and as many layers as fit when it does not; choosing
/// Processor turns Auto off and gives no device; a model picked anew turns Auto on again and is fitted.
#[test]
fn p3_g16_auto_fits_the_layers_to_the_card_and_a_choice_or_a_new_model_says_otherwise() {
    let dir = scratch("g16");
    let models = dir.join("models");
    // 16 GiB of weights in 40 layers, and a small one
    gguf(&models, "Big-Q4_K_M.gguf", 40, 32768, 16 * GIB);
    gguf(&models, "Small-Q4_K_M.gguf", 16, 32768, GIB);
    let mut app = app_in(&dir, "llamacpp", 64 * GIB);
    let big = app
        .models
        .iter()
        .position(|m| m.filename.starts_with("Big"))
        .unwrap();
    let small = app
        .models
        .iter()
        .position(|m| m.filename.starts_with("Small"))
        .unwrap();
    app.selected = big;
    app.refresh_facts();
    app.values.set("ctx_size", 4096i64);
    with_cards(
        &mut app,
        vec![card("CUDA0", "RTX 4070 Ti", 12, 12, DeviceKind::Discrete)],
        false,
    );
    // (11 GiB - the work) / ((16 GiB + 4K of context) / 40)
    let Place::Mixed(n) = app.place() else {
        panic!("part of it on the card: {:?}", app.place())
    };
    assert!((24..=26).contains(&n), "{n} layers");
    assert_eq!(after(&args(&app), "--n-gpu-layers"), Some(n.to_string()));
    assert!(app.gpu_auto);

    // the person chooses the processor: no layers, no device, and Auto is off
    app.choose_place(Place::Processor);
    let a = args(&app);
    assert_eq!(after(&a, "--n-gpu-layers").as_deref(), Some("0"));
    assert_eq!(after(&a, "--device").as_deref(), Some("none"));
    assert_eq!(a.iter().filter(|w| *w == "--device").count(), 1, "{a:?}");
    assert!(!app.gpu_auto);
    app.auto_fit();
    assert_eq!(
        app.place(),
        Place::Processor,
        "a choice is kept while Auto is off"
    );

    // another model: Auto again, and the small one goes on the card whole
    app.selected = small;
    app.apply_model_defaults();
    assert!(app.gpu_auto);
    assert_eq!(app.place(), Place::Card);
    assert_eq!(after(&args(&app), "--n-gpu-layers").as_deref(), Some("999"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// G17: built-in graphics beside a card of its own are left out (`--device` names the card); every card ticked is
/// llama.cpp's own choice again.
#[test]
fn p3_g17_built_in_graphics_beside_a_card_are_left_out() {
    let dir = scratch("g17");
    gguf(&dir.join("models"), "Small-Q4_K_M.gguf", 16, 32768, GIB);
    let mut app = app_in(&dir, "llamacpp", 32 * GIB);
    with_cards(
        &mut app,
        vec![
            card(
                "Vulkan0",
                "Radeon RX 7900 XTX",
                24,
                23,
                DeviceKind::Discrete,
            ),
            card(
                "Vulkan1",
                "UHD Graphics 770",
                15,
                15,
                DeviceKind::Integrated,
            ),
        ],
        false,
    );
    assert_eq!(after(&args(&app), "--device").as_deref(), Some("Vulkan0"));
    let ticked: Vec<String> = app.ticked_cards().into_iter().map(|c| c.id).collect();
    assert_eq!(ticked, ["Vulkan0"]);

    // both ticked: no --device, llama.cpp takes every card
    app.values.clear("devices");
    assert!(!args(&app).iter().any(|a| a == "--device"));
    assert_eq!(app.ticked_cards().len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// G18: two cards split by rows, with a share of each, are what Start gives; the meters share the model the same way.
#[test]
fn p3_g18_two_cards_split_by_rows_with_a_share_reach_the_command_line_and_the_meters() {
    let dir = scratch("g18");
    gguf(&dir.join("models"), "Mid-Q4_K_M.gguf", 32, 32768, 10 * GIB);
    let mut app = app_in(&dir, "llamacpp", 64 * GIB);
    with_cards(
        &mut app,
        vec![
            card("CUDA0", "RTX 3090 (1)", 24, 24, DeviceKind::Discrete),
            card("CUDA1", "RTX 3090 (2)", 24, 24, DeviceKind::Discrete),
        ],
        true,
    );
    app.values.set("split_mode", "row");
    app.values.set("tensor_split", "60,40");
    let a = args(&app);
    assert_eq!(after(&a, "--split-mode").as_deref(), Some("row"));
    assert_eq!(after(&a, "--tensor-split").as_deref(), Some("60,40"));
    let (cards, memory) = app.takes().expect("what it takes");
    assert_eq!(cards.len(), 2);
    let model = (10 * GIB) as f64;
    assert!(((cards[0].1.model as f64) / model - 0.6).abs() < 0.01);
    assert!(((cards[1].1.model as f64) / model - 0.4).abs() < 0.01);
    assert_eq!(memory.model, 0, "all of it is on the cards");
    let _ = std::fs::remove_dir_all(&dir);
}

/// G19: names of cards this build of llama.cpp does not list (another build, a card taken out) are left out, and the
/// share with them, so that Start is not refused for them.
#[test]
fn p3_g19_card_names_the_build_does_not_list_are_left_out() {
    let dir = scratch("g19");
    gguf(&dir.join("models"), "Small-Q4_K_M.gguf", 16, 32768, GIB);
    let mut app = app_in(&dir, "llamacpp", 32 * GIB);
    app.values.set("devices", "CUDA0,CUDA1");
    app.values.set("tensor_split", "3,1");
    with_cards(
        &mut app,
        vec![card(
            "Vulkan0",
            "Radeon RX 7900 XTX",
            24,
            23,
            DeviceKind::Discrete,
        )],
        false,
    );
    assert_eq!(app.values.text("devices"), None);
    assert_eq!(app.values.text("tensor_split"), None);
    let a = args(&app);
    assert!(
        !a.iter().any(|w| w == "--device" || w == "--tensor-split"),
        "{a:?}"
    );

    // one of two still there is kept
    app.values.set("devices", "Vulkan0,Vulkan5");
    app.settle_devices();
    assert_eq!(app.values.text("devices"), Some("Vulkan0"));
    // and a build with no graphics at all (OpenVINO on the processor) leaves the layers as they are
    with_cards(
        &mut app,
        vec![card(
            "OPENVINO0",
            "Core i7-10700",
            31,
            31,
            DeviceKind::Processor,
        )],
        false,
    );
    assert!(app.graphics_cards().is_empty());
    assert_eq!(app.values.text("devices"), None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// G20: a configuration keeps the context, the layers and the split, and not the cards or their shares; chosen again
/// it is fitted to this computer's card; and the runner plans the file the window saved with what it says.
#[test]
fn p3_g20_a_configuration_keeps_what_travels_is_fitted_here_and_the_runner_takes_it() {
    let dir = scratch("g20");
    let model = gguf(&dir.join("models"), "Mid-Q4_K_M.gguf", 32, 131_072, 4 * GIB);
    let mut app = app_in(&dir, "llamacpp", 32 * GIB);
    with_cards(
        &mut app,
        vec![
            card("CUDA0", "RTX 3090 (1)", 24, 24, DeviceKind::Discrete),
            card("CUDA1", "RTX 3090 (2)", 24, 24, DeviceKind::Discrete),
        ],
        true,
    );
    app.choose_length(65536);
    app.choose_place(Place::Mixed(20));
    app.values.set("split_mode", "row");
    app.values.set("devices", "CUDA1");
    app.values.set("tensor_split", "60,40");
    let id = app.save_as_configuration("Long rows").expect("saved");
    let file = dir.join("config/profiles").join(format!("{id}.toml"));
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("ctx_size = 65536"), "{text}");
    assert!(text.contains("n_gpu_layers = 20"), "{text}");
    assert!(text.contains("split_mode = \"row\""), "{text}");
    assert!(
        !text.contains("devices") && !text.contains("tensor_split"),
        "{text}"
    );

    // the runner plans the file as the window saved it
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = crate::cli::run_with(
        &[
            "plan",
            "--json",
            "--configuration",
            file.to_str().unwrap(),
            "--model",
            model.to_str().unwrap(),
            "--binary",
            "/x/llama-server",
        ]
        .map(String::from),
        &ConfigDirs::default(),
        &mut out,
        &mut err,
    );
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    let plan: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let planned: Vec<String> = plan["units"][0]["start"][0]["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["value"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(after(&planned, "--ctx-size").as_deref(), Some("65536"));
    assert_eq!(after(&planned, "--n-gpu-layers").as_deref(), Some("20"));
    assert_eq!(after(&planned, "--split-mode").as_deref(), Some("row"));

    // other numbers, then the configuration chosen again: its numbers, fitted to this computer's cards (the model fits
    // whole on them, so all its layers go there)
    app.choose_length(8192);
    app.values.clear("split_mode");
    app.choose_configuration(&id);
    assert_eq!(app.values.int("ctx_size"), Some(65536));
    assert_eq!(app.values.text("split_mode"), Some("row"));
    assert!(app.gpu_auto);
    assert_eq!(app.place(), Place::Card);
    let _ = std::fs::remove_dir_all(&dir);
}

/// G21: with Ollama, Graphics is its own switch: Processor hides the cards from it.
#[test]
fn p3_g21_ollamas_processor_hides_the_cards_from_it() {
    let dir = scratch("g21");
    let mut app = app_in(&dir, "ollama", 16 * GIB);
    assert!(app.form().and_then(|f| f.field("gpu").cloned()).is_some());
    app.values.set("gpu", "off");
    let plan = app.plan_to_show().expect("a plan");
    let env: Vec<String> = plan.units[0]
        .start
        .iter()
        .filter_map(|s| match s {
            Start::Spawn(spawn) => Some(spawn.env.iter().map(|e| e.name.clone())),
            Start::Attach(_) => None,
        })
        .flatten()
        .collect();
    assert!(env.iter().any(|n| n == "CUDA_VISIBLE_DEVICES"), "{env:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// G22: the port is not in the panel: it is on the Settings' page of how it starts, and the port taken's "Go to the
/// setting" opens that page.
#[test]
fn p3_g22_the_port_is_on_the_settings_page_of_how_it_starts() {
    let dir = scratch("g22");
    gguf(&dir.join("models"), "Small-Q4_K_M.gguf", 16, 32768, GIB);
    let mut app = app_in(&dir, "llamacpp", 16 * GIB);
    assert!(!app.panel_shows_field("port"));
    assert!(app.panel_shows_field("ctx_size") && app.panel_shows_field("n_gpu_layers"));
    let elsewhere: Vec<String> = app
        .primary_fields_elsewhere()
        .into_iter()
        .map(|f| f.id)
        .collect();
    assert_eq!(
        elsewhere,
        ["port", "n_gpu_layers"],
        "the port, and the layers as a number"
    );
    app.invoke("focus_field:port");
    assert!(app.show_settings);
    assert_eq!(
        app.settings_page,
        super::settings_window::SettingsPage::Start
    );
    // the context opens the panel, at Other…
    app.invoke("focus_field:ctx_size");
    assert!(app.panel_open && app.change_custom);
    let _ = std::fs::remove_dir_all(&dir);
}

/// G23: part of the model on the card: the meters say that part on the card and the rest in the memory.
#[test]
fn p3_g23_part_on_the_card_is_shared_between_the_cards_meter_and_the_memorys() {
    let dir = scratch("g23");
    gguf(&dir.join("models"), "Big-Q4_K_M.gguf", 40, 32768, 16 * GIB);
    let mut app = app_in(&dir, "llamacpp", 64 * GIB);
    with_cards(
        &mut app,
        vec![card("CUDA0", "RTX 4070 Ti", 12, 12, DeviceKind::Discrete)],
        false,
    );
    app.choose_place(Place::Mixed(20));
    let (cards, memory) = app.takes().expect("what it takes");
    let half = 8 * GIB;
    assert!(
        cards[0].1.model.abs_diff(half) < GIB / 100,
        "{:?}",
        cards[0].1
    );
    assert!(memory.model.abs_diff(half) < GIB / 100, "{memory:?}");
    assert!(app
        .place_words()
        .unwrap()
        .starts_with("20 of 40 layers on the RTX 4070 Ti"));
    assert_eq!(app.speed(), Some(crate::hardware::SpeedTier::Medium));
    let _ = std::fs::remove_dir_all(&dir);
}

/// G26 (C215): Card + processor reaches at least as far as the processor alone, since the card's room is added to the
/// memory's: its dots say whether a length fits split, the layers on the card follow the length chosen, and its speed
/// goes by the share of the layers on the card. The suggested length still goes by the card alone.
#[test]
fn p3_g26_card_and_processor_fits_what_either_does_and_its_layers_follow_the_length() {
    let dir = scratch("g26");
    // 6 GiB of weights in 32 layers: 16K of context is 2 GiB, 128K is 16, 256K is 32
    gguf(&dir.join("models"), "Mid-Q4_K_M.gguf", 32, 262_144, 6 * GIB);
    let mut app = app_in(&dir, "llamacpp", 60 * GIB);
    app.values.set("ctx_size", 16384i64);
    with_cards(
        &mut app,
        vec![card(
            "ROCm0",
            "Radeon RX 6700 XT",
            12,
            12,
            DeviceKind::Discrete,
        )],
        false,
    );
    assert_eq!(app.place(), Place::Card);
    // on the card whole: 64K is more than its 11 GiB
    assert_eq!(app.fit(32768), Some(Fit::Good));
    assert_eq!(app.fit(65536), Some(Fit::Partly));
    // the processor alone: 128K in half of the memory, 256K tightly
    app.choose_place(Place::Processor);
    assert_eq!(app.fit(131072), Some(Fit::Good));
    assert_eq!(app.fit(262144), Some(Fit::Partly));
    // split: the card's 11 GiB and half the memory's 30 hold 256K (38.5 GiB) too
    app.choose_place(Place::Mixed(31));
    assert_eq!(app.fit(16384), Some(Fit::Good));
    assert_eq!(app.fit(131072), Some(Fit::Good));
    assert_eq!(app.fit(262144), Some(Fit::Good));
    assert_eq!(app.fit(1 << 20), Some(Fit::TooBig));
    assert_eq!(
        app.speed(),
        Some(crate::hardware::SpeedTier::Fast),
        "31 of 32 on the card"
    );

    // 128K chosen: 31 layers and their context would not fit on the card, so as many go as fit, (11 - 0.5) / (22 / 32)
    app.choose_length(131072);
    assert!(
        !app.gpu_auto,
        "the person's choice of where it runs is kept"
    );
    assert_eq!(app.place(), Place::Mixed(15));
    assert_eq!(after(&args(&app), "--n-gpu-layers").as_deref(), Some("15"));
    assert_eq!(
        app.speed(),
        Some(crate::hardware::SpeedTier::Medium),
        "15 of 32"
    );
    // and back to 16K: all but one fit again
    app.choose_length(16384);
    assert_eq!(app.place(), Place::Mixed(31));

    // an 8 GiB card: the suggestion is what runs on it whole (4K), not the 32K a split holds
    with_cards(
        &mut app,
        vec![card("ROCm0", "Radeon RX 6600", 8, 8, DeviceKind::Discrete)],
        false,
    );
    assert_eq!(app.fit(32768), Some(Fit::Good));
    assert_eq!(app.suggested_context(), Some(4096));
    let _ = std::fs::remove_dir_all(&dir);
}

/// G24: the panel's Tools is closed at first and says how many servers are on; opened, it names each with a switch.
#[test]
fn p3_g24_the_panels_tools_say_how_many_are_on_and_open_to_a_switch_each() {
    let dir = scratch("g24");
    gguf(&dir.join("models"), "Small-Q4_K_M.gguf", 16, 32768, GIB);
    let mut app = app_in(&dir, "llamacpp", 16 * GIB);
    // llama.cpp is there (a file is what the window looks for), and its cards are known (none)
    std::fs::write(&app.binary, "").unwrap();
    with_cards(&mut app, Vec::new(), false);
    app.set_mcp_text(
        r#"{"mcpServers":{"files":{"command":"/nowhere/files"},"notes":{"command":"/nowhere/notes","disabled":true}}}"#
            .to_string(),
        &egui::Context::default(),
    );
    let ctx = egui::Context::default();
    let said = painted(&mut app, &ctx);
    assert!(said.contains("1 of 2 on"), "{said}");
    assert!(
        !said.lines().any(|l| l == "notes"),
        "closed at first: {said}"
    );
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("panel_tools_open"), true));
    let said = painted(&mut app, &ctx);
    assert!(
        said.lines().any(|l| l == "files") && said.lines().any(|l| l == "notes"),
        "{said}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// G25: the cards are asked of the installed program itself (`--list-devices`), once for each program.
#[cfg(unix)]
#[test]
fn p3_g25_the_cards_are_asked_of_the_installed_llamacpp() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("g25");
    gguf(&dir.join("models"), "Small-Q4_K_M.gguf", 16, 32768, GIB);
    let mut app = app_in(&dir, "llamacpp", 16 * GIB);
    std::fs::write(
        &app.binary,
        "#!/bin/sh\n[ \"$1\" = --list-devices ] || exit 1\necho asked >> \"$(dirname \"$0\")/asked\"\n\
         echo 'Available devices:'\necho '  CUDA0: NVIDIA GeForce RTX 4090 (24210 MiB, 23608 MiB free)'\n",
    )
    .unwrap();
    std::fs::set_permissions(&app.binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let ctx = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while app.graphics_cards().is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "the cards were never known"
        );
        app.poll_devices(&ctx);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(app.graphics_cards()[0].name, "RTX 4090");
    for _ in 0..5 {
        app.poll_devices(&ctx);
    }
    let asked = std::fs::read_to_string(dir.join("asked")).unwrap();
    assert_eq!(asked.lines().count(), 1, "asked once");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The words the whole window paints, after a few frames.
fn painted(app: &mut App, ctx: &egui::Context) -> String {
    fn walk(shape: &egui::epaint::Shape, into: &mut Vec<String>) {
        match shape {
            egui::epaint::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, into)),
            egui::epaint::Shape::Text(t) => into.push(t.galley.text().to_string()),
            _ => {}
        }
    }
    let mut texts = Vec::new();
    for _ in 0..3 {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 900.0),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| app.show_window(ui));
        output.textures_delta.clear();
        texts.clear();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut texts);
        }
    }
    texts.join("\n")
}
