//! `fael chunk ask|reply|ask withdraw` and the owner's `chunk answer <contract>` (PLAN-fael-parallel
//! §2): the CLI over `fael_core::plan` contracts. All of it is plans.db; no run of the caller ends.

use crate::Args;
use fael_core::plan::{Ask, Here, Reply, Store};

/// Is this command a contract's? `answer` is one only when its id names no chunk.
pub(crate) fn takes(rest: &[&str], s: &Store) -> bool {
    match rest {
        ["ask", ..] | ["reply", ..] => true,
        ["answer", id, _] => !s.is_chunk(id) && s.is_contract(id),
        _ => false,
    }
}

pub(crate) fn run(a: &Args, rest: &[&str], s: &mut Store, here: &Here) -> Result<(), String> {
    match rest {
        ["ask", "withdraw", id] => {
            a.only("chunk ask withdraw", &["run"])?;
            s.withdraw(id, here)?;
            println!("contract {id} → withdrawn");
        }
        ["ask", to, want] => {
            a.only("chunk ask", &["returns", "check", "run"])?;
            let returns = a.one("returns").ok_or(
                "rejected: chunk ask needs --returns \"<shape>\" — what you will code against",
            )?;
            let id = s.ask(to, want, &returns, a.one("check").as_deref(), here)?;
            println!(
                "contract {id} → open · {to} agrees or counters with `fael chunk reply {id} agree|counter`; your run goes on"
            );
        }
        ["reply", id, "agree"] | ["reply", id, "agree", _] => {
            a.only("chunk reply", &["run"])?;
            let note = rest.get(3).copied();
            say(id, s.reply(id, Reply::Agree(note), here)?);
        }
        ["reply", id, "counter", shape, why] => {
            a.only("chunk reply", &["run"])?;
            say(id, s.reply(id, Reply::Counter { shape, why }, here)?);
        }
        ["answer", id, shape] => {
            a.only("chunk answer", &[])?;
            s.answer_contract(id, shape, here.now)?;
            say(id, "agreed".into());
        }
        _ => {
            return Err(
                "rejected: fael chunk ask <to> \"<want>\" --returns \"<shape>\" [--check cmd] · ask withdraw <id> · reply <id> agree [note] | counter \"<shape>\" \"<why>\""
                    .into(),
            );
        }
    }
    Ok(())
}

fn say(id: &str, state: String) {
    let more = if state == "owner" {
        format!(" — waits on the owner: `fael chunk answer {id} \"<shape>\"`")
    } else {
        String::new()
    };
    println!("contract {id} → {state}{more}");
}

/// The lines `chunk show` adds for a chunk's asks, in and out. The check is printed, never run.
pub(crate) fn lines(asks: &[Ask]) -> Vec<String> {
    let mut out = Vec::new();
    for k in asks {
        let (arrow, who) = if k.dir == "out" {
            ("→", "asked")
        } else {
            ("←", "asked by")
        };
        out.push(format!(
            "ask {} {arrow} {} ({who}) · {} · {} → {}",
            k.id, k.chunk, k.state, k.want, k.returns
        ));
        if let Some(c) = &k.counter {
            out.push(format!("  counter ({} round(s)): {c}", k.rounds));
        }
        if let Some(c) = &k.check {
            out.push(format!("  check: {c}"));
        }
    }
    out
}
