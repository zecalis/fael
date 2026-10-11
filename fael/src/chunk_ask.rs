//! `fael chunk ask|reply|ask withdraw` (PLAN-fael-parallel §2): the CLI over `fael_core::plan`
//! contracts. All of it is plans.db; no run of the caller ends. The holder's answer is final.

use crate::Args;
use fael_core::plan::{Ask, Here, Reply, Store};

/// Is this command a contract's?
pub(crate) fn takes(rest: &[&str]) -> bool {
    matches!(rest, ["ask", ..] | ["reply", ..])
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
            let (id, refused) = s.ask(to, want, &returns, a.one("check").as_deref(), here)?;
            match refused {
                Some(why) => println!("contract {id} → refused: {why}"),
                None => println!(
                    "contract {id} → open · {to} answers with `fael chunk reply {id} agree|refuse`, final; your run goes on"
                ),
            }
        }
        ["reply", id, "agree"] | ["reply", id, "agree", _] => {
            a.only("chunk reply", &["returns", "run"])?;
            let returns = a.one("returns");
            let r = Reply::Agree {
                note: rest.get(3).copied(),
                returns: returns.as_deref(),
            };
            println!("contract {id} → {}", s.reply(id, r, here)?);
        }
        ["reply", id, "refuse", why] => {
            a.only("chunk reply", &["run"])?;
            println!("contract {id} → {}", s.reply(id, Reply::Refuse(why), here)?);
        }
        _ => {
            return Err(
                "rejected: fael chunk ask <to> \"<want>\" --returns \"<shape>\" [--check cmd] · ask withdraw <id> · reply <id> agree [note] [--returns \"<shape>\"] | refuse \"<why>\""
                    .into(),
            );
        }
    }
    Ok(())
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
        if let Some(r) = &k.reason {
            out.push(format!("  refused: {r}"));
        }
        if let Some(c) = &k.check {
            out.push(format!("  check: {c}"));
        }
    }
    out
}
