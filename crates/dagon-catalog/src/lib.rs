//! The registry of every DAGON state machine's catalog, and the documents
//! generated from it and from the port catalog. Code that reads events
//! (the CLI, host tools) depends on this crate, not on `dagon-events`.
//!
//! Every workspace crate that depends on `dagon-events` (directly or
//! through another crate, other than through this one) declares its
//! machines in one `dagon_events::state_machine!` at its root (none is
//! allowed), which defines its `MACHINES` static; [`REGISTRY`] lists each
//! crate's `MACHINES` by path, and a test fails while such a crate is
//! missing from it. A crate that depends on this one (the CLI) reads the
//! catalogs and declares no machine. The tests also check the layers'
//! event codes and regenerate the site's Events page; its Ports page and
//! the port table of its Lifecycle events page come from
//! `dagon_events::PORTS`. Regenerate them with
//! `DAGON_BLESS=1 cargo test -p dagon-catalog --test docs`.
#![forbid(unsafe_code)]

use std::fmt::Write;

use dagon_events::Machines;
pub use dagon_events::EVENT_FORMAT;

/// The boot kind that reports an extension call over its time limit: the
/// boot core delivers it to its observers once per such call.
pub const BOOT_OVERRAN_KIND: &str = dagon_events::boot_extensions::ExtensionOverran::ENTRY.kind;
/// What host tools validate against: the catalog types, the ports and the
/// decision points.
pub use dagon_events::{
    Catalog, DecisionPoint, Entry, Layer, Port, PortDefault, Service, Trust, DECISIONS,
    DECISION_BOUND_MS, PORTS,
};

/// `registry![a, b]` is `&[&a::MACHINES, &b::MACHINES]`: each entry is a
/// crate's own `MACHINES` static, never a value built here.
macro_rules! registry {
    ($($krate:ident),* $(,)?) => {
        &[$(&$krate::MACHINES),*]
    };
}

/// Each machine-declaring crate's `MACHINES`, one entry per crate.
pub static REGISTRY: &[&Machines] = registry![dagon_events, dagon_boot, dagon_agent, dagon_sim];

/// Every machine of every layer.
pub fn machines() -> Vec<Catalog> {
    REGISTRY
        .iter()
        .flat_map(|m| m.catalogs().iter().copied())
        .collect()
}

/// Faults across machines that one machine's declaration cannot see: two
/// kinds of a layer with one code or one name, two machines of a layer
/// with one name.
pub fn conflicts(machines: &[Catalog]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, a) in machines.iter().enumerate() {
        for b in &machines[i + 1..] {
            if a.layer != b.layer {
                continue;
            }
            if a.machine == b.machine {
                out.push(format!(
                    "{}: two machines named {}",
                    a.layer.name(),
                    a.machine
                ));
            }
            for x in a.entries {
                for y in b.entries {
                    if x.code == y.code {
                        out.push(format!(
                            "{}: code {:#04x} is {}.{} and {}.{}",
                            a.layer.name(),
                            x.code,
                            a.machine,
                            x.kind,
                            b.machine,
                            y.kind
                        ));
                    }
                    if x.kind == y.kind {
                        out.push(format!(
                            "{}.{} is emitted by {} and {}",
                            a.layer.name(),
                            x.kind,
                            a.machine,
                            b.machine
                        ));
                    }
                }
            }
        }
    }
    out
}

fn cell(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('|', "\\|")
}

fn observed(e: &Entry) -> String {
    let o = e.observe;
    [(o.live, "live"), (o.replay, "replay"), (o.host, "host")]
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, n)| *n)
        .collect::<Vec<_>>()
        .join(", ")
}

fn payload(e: &Entry) -> String {
    if e.payload.is_empty() {
        return "–".into();
    }
    e.payload
        .iter()
        .map(|f| format!("`{}: {}` ({} B)", f.name, f.ty, f.bytes))
        .collect::<Vec<_>>()
        .join(", ")
}

fn notes(e: &Entry) -> String {
    let mut n = Vec::new();
    if e.flags.action {
        n.push("action");
    }
    if e.flags.retired {
        n.push("retired");
    }
    n.join(", ")
}

/// The language of a generated page. Its headings and fixed text follow it;
/// what the catalogs themselves say (doc comments, notes) is English in the
/// code, and the Japanese page takes it through the given table lookup.
#[derive(Clone, Copy)]
pub enum Lang<'a> {
    /// The site's root (`site/src/content/docs/reference/`).
    Ja(&'a dyn Fn(&str) -> String),
    /// `site/src/content/docs/en/reference/`.
    En,
}

/// What a catalog says, in `lang`, as a table cell.
fn said(lang: Lang<'_>, text: &str) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match lang {
        Lang::Ja(look_up) if !one_line.is_empty() => cell(&look_up(&one_line)),
        _ => cell(&one_line),
    }
}

/// The site's Events page in `lang`.
pub fn events_md(machines: &[Catalog], lang: Lang<'_>) -> String {
    let mut s = String::new();
    match lang {
        Lang::Ja(_) => events_intro_ja(&mut s),
        Lang::En => events_intro_en(&mut s),
    }
    for d in DECISIONS {
        let _ = writeln!(
            s,
            "| `{}` | {} | {} | {} | {} | {} | {} |",
            d.id,
            d.layer.name(),
            said(lang, d.answers),
            said(lang, d.answered_by),
            said(lang, d.bound),
            said(lang, d.default),
            said(lang, d.never)
        );
    }
    for layer in Layer::ALL {
        let mine: Vec<_> = machines.iter().filter(|m| m.layer == layer).collect();
        if mine.is_empty() {
            continue;
        }
        let _ = match lang {
            Lang::Ja(_) => write!(s, "\n## レイヤー `{}`\n", layer.name()),
            Lang::En => write!(s, "\n## Layer `{}`\n", layer.name()),
        };
        for m in mine {
            machine_md(&mut s, m, lang);
        }
    }
    s
}

fn events_intro_ja(s: &mut String) {
    s.push_str(
        "---\ntitle: イベント\ndescription: DAGON が出すすべてのイベントです。\neditUrl: false\n---\n\n\
         :::note\n状態機械のカタログから生成しています。直接編集せず、\n\
         `DAGON_BLESS=1 cargo test -p dagon-catalog --test docs` でもう一度生成してください。\n:::\n\n\
         DAGON の状態の変化は、どれも状態機械の遷移で、遷移はどれもイベントを 1 つ出します。\n\
         このページは状態機械の宣言から作っているので、すべてのイベントが載っています。\n\n",
    );
    let _ = write!(
        s,
        "イベントの形式: {EVENT_FORMAT}（`dagon_events::EVENT_FORMAT`）。種類やペイロードのフィールドを足しても\n\
         番号は変わりません。名前を変える、消す、意味を変えるときは番号が変わります。読む側は、\n\
         知らない種類とフィールドを読み飛ばし、読めない形式の番号は受け付けません。\n\n"
    );
    s.push_str(
        "## 表の見方\n\n\
         - **種類**: `layer.kind`。ペイロードの型名を snake case にしたものです。\n\
         - **コード**: そのレイヤーの中での種類の番号です（`EventRecord.kind`）。\n\
         - **遷移**: イベントを出すメソッドと、移る前後の状態です（`*` はどの状態でも）。\n\
         - **ペイロード**: フィールドと、記録の中でのバイト数です（`EventRecord.payload`、順に並び、\n\
         \x20 合わせて 16 バイトまで）。\n\
         - **受け取り**: *live* は同じイメージの拡張が起きたときに、*replay* は起動時にアプリのエージェントが\n\
         \x20 `replayed: true` を付けて出し直したものを、*host* は CLI、serve のイベント、`--events`、\n\
         \x20 アクションが受け取ります。\n\
         - **備考**: *action* はブートのアクションプログラムを動かせます。*retired* はもう出さず、\n\
         \x20 次のイベントの形式まで残しています。\n\n\
         ## 判断点\n\n\
         ここに無いものは通知です。答えは、チェーンの順（公開元、ボードパッケージ、ライブラリ、利用者）で\n\
         最初のものが使われます。どの拡張も答えなければ、既定の値を使います。\n\n\
         | 判断 | レイヤー | 問い | 答える側 | 時間の上限 | 既定 | してはいけないこと |\n\
         |---|---|---|---|---|---|---|\n",
    );
}

fn events_intro_en(s: &mut String) {
    s.push_str(
        "---\ntitle: Events\ndescription: Every event DAGON emits, from the state machines' catalogs.\neditUrl: false\n---\n\n\
         :::note\nGenerated from the state machines' catalogs; do not edit. Regenerate with\n\
         `DAGON_BLESS=1 cargo test -p dagon-catalog --test docs`.\n:::\n\n\
         Every state change in DAGON is a transition of a state machine, and every\n\
         transition emits one event. This page is generated from the machines'\n\
         declarations, so it lists every event there is.\n\n",
    );
    let _ = write!(
        s,
        "Event format: {EVENT_FORMAT} (`dagon_events::EVENT_FORMAT`). Adding a kind or a payload\n\
         field keeps the number; renaming or removing one, or changing its meaning, moves\n\
         it. A reader ignores kinds and fields it does not know and refuses a format\n\
         number it does not read.\n\n"
    );
    s.push_str(
        "## Reading the tables\n\n\
         - **Kind**: `layer.kind`, the payload type's name in snake case.\n\
         - **Code**: the kind's number within its layer (`EventRecord.kind`).\n\
         - **Transition**: the method that emits it, and the states it moves between\n\
         \x20 (`*` is any state).\n\
         - **Payload**: the fields, each with its bytes in the owned record\n\
         \x20 (`EventRecord.payload`, in order, 16 bytes at most).\n\
         - **Observed**: *live* by extensions in the same image as it happens; *replay*\n\
         \x20 re-emitted by the app's agent at start with `replayed: true`; *host* by the\n\
         \x20 CLI, the serve event stream, `--events` and actions.\n\
         - **Notes**: *action*: a boot action program may run on it; *retired*: no longer\n\
         \x20 emitted, kept until the next event format.\n\n\
         ## Decision points\n\n\
         Everything not listed here is a notification. Each answer is the first one in\n\
         the chain's order (publisher, board package, libraries, user); when every\n\
         extension abstains, the default applies.\n\n\
         | Decision | Layer | Question | Answered by | Bound | Default | Never |\n\
         |---|---|---|---|---|---|---|\n",
    );
}

fn machine_md(s: &mut String, m: &Catalog, lang: Lang<'_>) {
    let (machine, initial, states_word, head) = match lang {
        Lang::Ja(_) => (
            "状態機械",
            "最初の状態",
            "状態",
            "| 種類 | コード | 遷移 | ペイロード | 受け取り | 備考 | 説明 |",
        ),
        Lang::En => (
            "Machine",
            "initial",
            "States",
            "| Kind | Code | Transition | Payload | Observed | Notes | Description |",
        ),
    };
    let _ = write!(s, "\n### {machine} `{}`\n\n", m.machine);
    if !m.doc.trim().is_empty() {
        let _ = write!(s, "{}\n\n", said(lang, m.doc));
    }
    let states: Vec<_> = m
        .states
        .iter()
        .map(|st| {
            if *st == m.initial {
                format!("`{st}` ({initial})")
            } else {
                format!("`{st}`")
            }
        })
        .collect();
    let _ = match lang {
        Lang::Ja(_) => write!(s, "{states_word}: {}。\n\n", states.join(", ")),
        Lang::En => write!(s, "{states_word}: {}.\n\n", states.join(", ")),
    };
    let _ = writeln!(s, "{head}\n|---|---|---|---|---|---|---|");
    for e in m.entries {
        let _ = writeln!(
            s,
            "| `{}.{}` | {:#04x} | `{}`: {} → {} | {} | {} | {} | {} |",
            m.layer.name(),
            e.kind,
            e.code,
            e.trigger,
            e.from.join(" \\| "),
            e.to,
            payload(e),
            observed(e),
            notes(e),
            said(lang, e.doc)
        );
    }
}

fn trust(p: &Port, lang: Lang<'_>) -> String {
    let t = match p.trust {
        Trust::Untrusted => "untrusted",
        Trust::Additive => "untrusted, additive",
        Trust::TrustedOnly => "trusted only",
        Trust::Never(_) => "not a port",
    };
    if p.note.is_empty() {
        t.into()
    } else {
        format!("{t} ({})", said(lang, p.note))
    }
}

/// The port's layers, in catalog order.
fn layer_names(p: &Port) -> String {
    let names: Vec<_> = p.layers.iter().map(|d| d.layer.name()).collect();
    names.join(", ")
}

/// The default per layer; layers with one default share an entry
/// (`boot, app: X; host: Y`), and a single default stands alone.
fn defaults(p: &Port, lang: Lang<'_>) -> String {
    let mut groups: Vec<(Vec<&str>, &str)> = Vec::new();
    for d in p.layers {
        match groups.iter_mut().find(|(_, v)| *v == d.default) {
            Some((names, _)) => names.push(d.layer.name()),
            None => groups.push((vec![d.layer.name()], d.default)),
        }
    }
    if let [(_, only)] = groups.as_slice() {
        return said(lang, only);
    }
    groups
        .iter()
        .map(|(names, v)| format!("{}: {}", names.join(", "), said(lang, v)))
        .collect::<Vec<_>>()
        .join("; ")
}

fn services(p: &Port) -> String {
    if p.services.is_empty() {
        return "–".into();
    }
    let names: Vec<_> = p.services.iter().map(|x| x.name()).collect();
    names.join(", ")
}

/// The port table of the design document (section 2.1), between its
/// generated-table markers.
pub fn design_ports_table(ports: &[Port], lang: Lang<'_>) -> String {
    let mut s = String::from(match lang {
        Lang::Ja(_) => {
            "| ポート | レイヤー | 既定 | 信頼の方針 | サービス |\n|---|---|---|---|---|\n"
        }
        Lang::En => {
            "| Port | Layers | Default | Trust policy | Services |\n|---|---|---|---|---|\n"
        }
    });
    for p in ports.iter().filter(|p| p.trust.is_port()) {
        let _ = writeln!(
            s,
            "| `{}` ({}) | {} | {} | {} | {} |",
            p.id,
            said(lang, p.what),
            layer_names(p),
            defaults(p, lang),
            trust(p, lang),
            services(p)
        );
    }
    s
}

const PORTS_INTRO_JA: &str =
    "---\ntitle: ポート\ndescription: DAGON の拡張が使うすべてのポートです。\neditUrl: false\n---\n\n\
     :::note\n`dagon_events::PORTS` から生成しています。直接編集せず、\n\
     `DAGON_BLESS=1 cargo test -p dagon-catalog --test docs` でもう一度生成してください。\n:::\n\n\
     ポートは、プラグインが置き換えられる DAGON の部品です。置き換えは、一覧のレイヤーで、リンクされた\n\
     イメージの中だけに効きます。既定は DAGON 自身の実装です。プラグインのマニフェストは、そのポートがある\n\
     レイヤーでだけポートを提供でき、1 つのイメージで 1 つのポートを提供できるのは 1 つだけです。\n\n\
     信頼の区分:\n\n\
     - *untrusted*: どのプラグインでも使えます。レイヤーのサンドボックスの中で、マニフェストで宣言した能力で\n\
     \x20 動き、DAGON は一覧のサービスを通してだけ呼びます。\n\
     - *untrusted, additive*: untrusted と同じで、足すだけです。DAGON 自身の実装は残ります。\n\
     - *trusted only*: 宣言して署名した、信頼する拡張だけが使えます。特権で動き、サービスは呼びません。\n\n\
     | ポート | 役割 | レイヤー | 既定 | 信頼 | サービス |\n\
     |---|---|---|---|---|---|\n";

const PORTS_NOT_JA: &str = "\n## ポートではない部品\n\n\
     信頼する拡張でも、次のものは置き換えられません。\n\n\
     | 部品 | 内容 | レイヤー | ポートではない理由 |\n\
     |---|---|---|---|\n";

/// The site's Ports page in `lang`.
pub fn ports_md(ports: &[Port], lang: Lang<'_>) -> String {
    let (intro, not_ports) = match lang {
        Lang::Ja(_) => (PORTS_INTRO_JA, PORTS_NOT_JA),
        Lang::En => (PORTS_INTRO_EN, PORTS_NOT_EN),
    };
    let mut s = String::from(intro);
    for p in ports.iter().filter(|p| p.trust.is_port()) {
        let _ = writeln!(
            s,
            "| `{}` | {} | {} | {} | {} | {} |",
            p.id,
            said(lang, p.what),
            layer_names(p),
            defaults(p, lang),
            trust(p, lang),
            services(p)
        );
    }
    s.push_str(not_ports);
    for p in ports {
        if let Trust::Never(why) = p.trust {
            let _ = writeln!(
                s,
                "| `{}` | {} | {} | {} |",
                p.id,
                said(lang, p.what),
                layer_names(p),
                said(lang, why)
            );
        }
    }
    s
}

const PORTS_INTRO_EN: &str =
        "---\ntitle: Ports\ndescription: Every port DAGON's extensions use, from the catalogs.\neditUrl: false\n---\n\n\
         :::note\nGenerated from `dagon_events::PORTS`; do not edit. Regenerate with\n\
         `DAGON_BLESS=1 cargo test -p dagon-catalog --test docs`.\n:::\n\n\
         A port is a part of DAGON that a plug-in replaces, in the layers listed, for\n\
         the image it is linked into; DAGON's own implementation is the default. A\n\
         plug-in's manifest may provide a port only in layers the port exists in, and\n\
         one image has at most one provider per port.\n\n\
         Trust:\n\n\
         - *untrusted*: any plug-in; it runs in the layer's sandbox with the\n\
         \x20 capabilities its manifest declares and calls DAGON only through the\n\
         \x20 services listed.\n\
         - *untrusted, additive*: as untrusted, and it only adds; DAGON's own\n\
         \x20 implementation stays.\n\
         - *trusted only*: only a trusted extension, declared and signed; it runs\n\
         \x20 privileged and calls no services.\n\n\
         | Port | What it does | Layers | Default | Trust | Services |\n\
         |---|---|---|---|---|---|\n";

const PORTS_NOT_EN: &str = "\n## Parts that are not ports\n\n\
         No plug-in replaces these, trusted or not.\n\n\
         | Part | What it is | Layers | Why not a port |\n\
         |---|---|---|---|\n";
