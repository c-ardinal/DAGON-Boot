//! Linking DAGON to an agent, as the user sees it: `dagon ai link`,
//! `unlink` and `key`, the status the page reads (GET /v1/agent), and the
//! check every AI command makes first. The consent itself is
//! [`crate::consent`], the keys [`crate::keystore`], the agents and their
//! ways [`crate::agents`].

use std::io::BufRead;
use std::path::Path;

use serde_json::{json, Value};

use crate::agents::{AgentDef, Agents, Transport, WayDef};
use crate::cli::{AgentWay, AiKeyCmd};
use crate::consent::{consent, home, link, unknown_version, unlink, Consent, NEVER_SENDS, SENDS};
use crate::keystore::Keys;

type LinkedWay<'a> = (Consent, &'a AgentDef, &'a WayDef);

/// The consent when its agent and way are still defined.
pub fn linked<'a>(home: &Path, agents: &'a Agents) -> Option<LinkedWay<'a>> {
    let c = consent(home)?;
    let (a, w) = agents.way(&c.agent, &c.way)?;
    Some((c.clone(), a, w))
}

/// GET /v1/agent: the agents and ways (with whether each can be used here:
/// program on PATH, key registered; never a key, path or command), the
/// consent, and what linking sends. A way that takes a key also says
/// whether one is registered (`key`). `available` and `provider` describe
/// the linked way, or the default one before linking.
pub fn status(home: Option<&Path>, agents: &Agents, keys: &Keys) -> Value {
    let linked = home.and_then(|h| linked(h, agents));
    let (a, w) = linked
        .as_ref()
        .map(|(_, a, w)| (*a, *w))
        .unwrap_or_else(|| agents.default_way());
    let list: Vec<Value> = agents
        .list
        .iter()
        .map(|a| {
            let ways: Vec<Value> = a
                .ways
                .iter()
                .map(|w| {
                    let key = w.transport.takes_key().then(|| keys.is_set(&a.id, &w.id));
                    json!({
                        "id": w.id,
                        "name": w.name,
                        "kind": w.transport.kind(),
                        "available": w.available(&a.id, keys),
                        "key": key,
                    })
                })
                .collect();
            json!({ "id": a.id, "name": a.name, "provider": a.provider, "ways": ways })
        })
        .collect();
    let (da, dw) = agents.default_way();
    json!({
        "available": w.available(&a.id, keys),
        "linked": linked.is_some(),
        "provider": { "id": a.id, "name": a.label() },
        "consent": linked.map(|(c, _, _)| json!({ "agent": c.agent, "way": c.way, "at": c.at })),
        "agents": list,
        "default": { "agent": da.id, "way": dw.id },
        "errors": agents.errors,
        // A consent file this dagon does not read: why (link again).
        "consent_error": home.and_then(unknown_version),
        "sends": SENDS,
        "never_sends": NEVER_SENDS,
    })
}

/// The error `dagon ai topology` and `ask` give before linking.
pub fn require_linked(home: &Path) -> Result<(), String> {
    if linked(home, &Agents::load(Some(home))).is_some() {
        Ok(())
    } else if let Some(e) = unknown_version(home) {
        Err(e)
    } else {
        Err(
            "DAGON is not linked to an agent; run dagon ai link first (consent is asked once)"
                .into(),
        )
    }
}

/// The consent text of the terminal: the page's linking dialog, worded for
/// the CLI.
fn consent_text(a: &AgentDef, w: &WayDef) -> String {
    let list = |l: &[&str]| l.join("、");
    let key = match &w.transport {
        Transport::Command(c) => format!(
            "キー: {}の設定とログインを使用します。DAGONはキーを参照しません",
            c.program
        ),
        Transport::MessagesHttp(_) => format!(
            "キー: OSの資格情報ストアに保管します（dagon ai key set --agent {} --way {}）。\
             キーは画面にも送信テキストにも出力しません",
            a.id, w.id
        ),
    };
    format!(
        "DAGONとの連携\n\n\
         DAGONは連携したエージェントへ構成について照会して回答します。\
         送信への同意は、この連携の際に1度だけ取得します。\
         連携後の質問は確認なしで送信します。\n\n\
         送信先: {}　接続方式: {}\n\
         {key}\n\
         送信内容: {}\n\
         送信対象外: {}\n\n\
         連携はdagon ai unlinkで随時解除可能です。解除すると同意も撤回します。",
        a.label(),
        w.name,
        list(&SENDS),
        list(&NEVER_SENDS),
    )
}

/// The agent and way `--agent` / `--way` name (default: the first agent,
/// its first way `default_way` accepts, else its first way).
pub(crate) fn chosen<'a>(
    agents: &'a Agents,
    sel: &AgentWay,
    default_way: impl Fn(&WayDef) -> bool,
) -> Result<(&'a AgentDef, &'a WayDef), String> {
    let (agent, way) = (sel.agent.as_deref(), sel.way.as_deref());
    let a = match agent {
        Some(id) => agents.list.iter().find(|a| a.id == id),
        None => agents.list.first(),
    }
    .ok_or_else(|| format!("no agent {}", agent.unwrap_or_default()))?;
    let w = match way {
        Some(id) => a.ways.iter().find(|w| w.id == id),
        None => a.ways.iter().find(|w| default_way(w)).or(a.ways.first()),
    }
    .ok_or_else(|| format!("{} has no way {}", a.id, way.unwrap_or_default()))?;
    Ok((a, w))
}

/// `dagon ai link`: shows what is sent and to
/// whom, and records consent on "y".
pub fn link_command(sel: &AgentWay) -> Result<(), String> {
    let home = home()?;
    let agents = Agents::load(Some(&home));
    for e in &agents.errors {
        eprintln!("warning: ~/.dagon/agents/{e}");
    }
    let (a, w) = chosen(&agents, sel, |_| true)?;
    if let Some((c, _, _)) = linked(&home, &agents) {
        if c.agent == a.id && c.way == w.id {
            println!("DAGONは連携済です。同意は{}に記録しました", c.at);
            return Ok(());
        }
    }
    eprintln!("{}\n", consent_text(a, w));
    eprint!("同意して連携しますか？ [y/N] ");
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    if !matches!(line.trim(), "y" | "Y" | "yes") {
        println!("連携しませんでした");
        return Ok(());
    }
    let c = link(&home, &a.id, &w.id)?;
    println!(
        "DAGONと連携しました。送信先{}（{}）と送信内容への同意を{}に記録しました",
        a.label(),
        w.name,
        c.at
    );
    if !w.available(&a.id, &Keys::system()) {
        println!(
            "この接続方式は現時点で使用不可です。{}",
            unavailable_hint(a, w)
        );
    }
    Ok(())
}

fn unavailable_hint(a: &AgentDef, w: &WayDef) -> String {
    match &w.transport {
        Transport::Command(c) => format!("{}をPATHから実行可能にしてください", c.program),
        Transport::MessagesHttp(_) => format!(
            "dagon ai key set --agent {} --way {}でキーを登録してください",
            a.id, w.id
        ),
    }
}

/// `dagon ai unlink`: removes the consent.
pub fn unlink_command() -> Result<(), String> {
    if unlink(&home()?)? {
        println!(
            "DAGONとの連携を解除し、同意を撤回しました。キーやログインはエージェント側で削除してください"
        );
    } else {
        println!("DAGONは未連携です");
    }
    Ok(())
}

/// The way `--agent` / `--way` name; default: the first agent, its first
/// way that takes a key.
fn key_way<'a>(agents: &'a Agents, sel: &AgentWay) -> Result<(&'a AgentDef, &'a WayDef), String> {
    let (a, w) = chosen(agents, sel, |w| w.transport.takes_key())?;
    if !w.transport.takes_key() {
        return Err(format!("{}/{} takes no key", a.id, w.id));
    }
    Ok((a, w))
}

/// `dagon ai key set|status|delete`.
pub fn key_command(cmd: &AiKeyCmd) -> Result<(), String> {
    let home = home()?;
    let agents = Agents::load(Some(&home));
    let keys = Keys::system();
    match cmd {
        AiKeyCmd::Set(sel) => {
            let (a, w) = key_way(&agents, sel)?;
            // Refused before anything is typed: no store, or no terminal to type into.
            keys.usable()?;
            if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
                return Err(
                    "the key is typed in a terminal (without echo); run this in one".into(),
                );
            }
            let prompt = format!("{}の{}のキー（表示されません）: ", a.name, w.name);
            let key = rpassword::prompt_password(prompt).map_err(|_| {
                "the key is read from a terminal (without echo); run this in one".to_string()
            })?;
            keys.set(&a.id, &w.id, &key)?;
            println!("{}/{}: 登録済", a.id, w.id);
            Ok(())
        }
        AiKeyCmd::Delete(sel) => {
            let (a, w) = key_way(&agents, sel)?;
            let removed = keys.delete(&a.id, &w.id)?;
            println!(
                "{}/{}: {}",
                a.id,
                w.id,
                if removed { "削除済み" } else { "未登録" }
            );
            Ok(())
        }
        AiKeyCmd::Status => {
            for a in &agents.list {
                for w in a.ways.iter().filter(|w| w.transport.takes_key()) {
                    let state = match keys.get(&a.id, &w.id) {
                        Ok(Some(_)) => "登録済".to_string(),
                        Ok(None) => "未登録".to_string(),
                        Err(e) => format!("未登録 ({e})"),
                    };
                    println!("{}/{} ({}, {}): {state}", a.id, w.id, a.name, w.name);
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_terminal_shows_the_same_lists_as_the_page() {
        let agents = Agents::load(None);
        let (a, cc) = agents.way("claude", "claude-code").unwrap();
        let (_, api) = agents.way("claude", "api").unwrap();
        let api_key = "dagon ai key set --agent claude --way api";
        for (w, key) in [(cc, "claudeの設定とログイン"), (api, api_key)] {
            let t = consent_text(a, w);
            for s in SENDS.iter().chain(&NEVER_SENDS) {
                assert!(t.contains(s), "{s}");
            }
            assert!(t.contains("Claude (Anthropic)") && t.contains(&w.name) && t.contains(key));
        }
        let any = |_: &WayDef| true;
        let pick = |agent: Option<&str>, way: Option<&str>| AgentWay {
            agent: agent.map(String::from),
            way: way.map(String::from),
        };
        assert_eq!(
            chosen(&agents, &pick(None, None), any).unwrap().1.id,
            "claude-code"
        );
        let key = |w: &WayDef| w.transport.takes_key();
        assert_eq!(chosen(&agents, &pick(None, None), key).unwrap().1.id, "api");
        assert_eq!(
            chosen(&agents, &pick(None, Some("api")), any).unwrap().1.id,
            "api"
        );
        assert!(chosen(&agents, &pick(Some("other"), None), any).is_err());
        assert!(chosen(&agents, &pick(None, Some("nope")), any).is_err());
    }
}
