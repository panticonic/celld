// Copyright 2026 Deno Land Inc. Apache-2.0 license.

// The secrets CLI reads operator input: a value, a file and a key file.
#![allow(clippy::disallowed_methods)]

//! `celld secret` — store the values of per-class secrets.
//!
//! ```text
//! celld secret keygen
//! celld secret put SCRIPT NAME [--value V | --file PATH | --pipe] --bucket B
//! celld secret list SCRIPT --bucket B
//! celld secret delete SCRIPT NAME --bucket B
//! ```
//!
//! A deployment declares which secrets a Durable Object class sees
//! (`celld.durable_objects.<Class>.secrets`); this command stores the values,
//! sealed under an operator-held key, apart from every deployment. See
//! `docs/fork/class-secrets.md` and [`crate::secrets`].

use crate::cli_options::{FleetFlags, FLEET_HELP};
use crate::cli_output::{Format, Output};
use crate::note;
use crate::secrets::{self, SecretsKey};
use anyhow::{anyhow, bail, Context};
use std::io::Read;
use std::path::PathBuf;

enum Source {
    Value(String),
    File(PathBuf),
    Pipe,
}

enum Verb {
    Keygen,
    Put {
        script: String,
        name: String,
        source: Source,
    },
    List {
        script: String,
    },
    Delete {
        script: String,
        name: String,
    },
}

struct Command {
    verb: Verb,
    fleet: FleetFlags,
    key_file: Option<PathBuf>,
    json: bool,
}

fn parse(arguments: Vec<String>) -> anyhow::Result<Option<Command>> {
    let mut arguments = arguments.into_iter();
    let verb = match arguments.next().as_deref() {
        None | Some("--help" | "-h" | "help") => return Ok(None),
        Some(verb @ ("keygen" | "put" | "list" | "delete")) => verb.to_string(),
        Some(other) => bail!("unknown `celld secret` subcommand: {other}"),
    };
    let mut positional = Vec::new();
    let mut source = None;
    let mut fleet = FleetFlags::default();
    let mut key_file = None;
    let mut json = false;
    while let Some(argument) = arguments.next() {
        let mut value = |flag: &str| {
            arguments
                .next()
                .ok_or_else(|| anyhow!("{flag} requires a value"))
        };
        match argument.as_str() {
            "--value" => source = Some(Source::Value(value("--value")?)),
            "--file" => source = Some(Source::File(PathBuf::from(value("--file")?))),
            "--pipe" => source = Some(Source::Pipe),
            "--secrets-key-file" => key_file = Some(PathBuf::from(value("--secrets-key-file")?)),
            "--json" => json = true,
            "--help" | "-h" => return Ok(None),
            flag if flag.starts_with("--") => {
                if !fleet.consume(flag, &mut |name| value(name))? {
                    bail!("unknown option for `celld secret {verb}`: {flag}");
                }
            }
            _ => positional.push(argument),
        }
    }
    let mut positional = positional.into_iter();
    let mut next = |what: &str| {
        positional
            .next()
            .ok_or_else(|| anyhow!("celld secret {verb} needs {what}"))
    };
    let verb = match verb.as_str() {
        "keygen" => Verb::Keygen,
        "put" => Verb::Put {
            script: next("a Worker name")?,
            name: next("a secret name")?,
            // With no source, a value on a terminal is read from the prompt
            // line of stdin: `--pipe` says so explicitly.
            source: source.unwrap_or(Source::Pipe),
        },
        "list" => Verb::List {
            script: next("a Worker name")?,
        },
        _ => Verb::Delete {
            script: next("a Worker name")?,
            name: next("a secret name")?,
        },
    };
    if let Some(extra) = positional.next() {
        bail!("celld secret got an unexpected argument: {extra}");
    }
    Ok(Some(Command {
        verb,
        fleet,
        key_file,
        json,
    }))
}

fn read_value(source: Source) -> anyhow::Result<String> {
    let bytes = match source {
        Source::Value(value) => value.into_bytes(),
        Source::File(path) => {
            std::fs::read(&path).with_context(|| format!("read {}", path.display()))?
        }
        Source::Pipe => {
            let mut bytes = Vec::new();
            std::io::stdin()
                .read_to_end(&mut bytes)
                .context("read the secret from standard input")?;
            // A value piped from `echo` ends in the newline `echo` adds, which
            // is never part of a credential.
            if bytes.last() == Some(&b'\n') {
                bytes.pop();
            }
            bytes
        }
    };
    String::from_utf8(bytes).map_err(|_| anyhow!("a secret must be UTF-8 text"))
}

pub async fn run(arguments: Vec<String>) -> anyhow::Result<()> {
    let Some(command) = parse(arguments)? else {
        print_help();
        return Ok(());
    };
    if let Verb::Keygen = command.verb {
        // The one line a key is written as, for CELLD_SECRETS_KEY or a file.
        let mut out = Output::new(if command.json {
            Format::Json
        } else {
            Format::Text
        });
        out.row(&KeyLine(SecretsKey::generate().to_base64()))?;
        return out.finish();
    }
    let storage = command.fleet.clone().resolve("celld secret")?;
    let bucket = storage.open().await?;
    let mut out = Output::new(if command.json {
        Format::Json
    } else {
        Format::Text
    });
    match command.verb {
        Verb::Keygen => unreachable!("answered above"),
        Verb::Put {
            script,
            name,
            source,
        } => {
            let key = match &command.key_file {
                Some(path) => SecretsKey::from_file(path)?,
                None => secrets::key_from_environment()?.ok_or_else(|| {
                    anyhow!(
                        "celld secret put needs the secrets key: set {} or {}, or pass \
                         --secrets-key-file (`celld secret keygen` makes one)",
                        secrets::KEY_ENV,
                        secrets::KEY_FILE_ENV
                    )
                })?,
            };
            let value = read_value(source)?;
            secrets::put(&bucket, &key, &script, &name, value.as_bytes()).await?;
            note!(
                "stored secret {name} for {script} (takes effect when a node next loads the \
                 deployment)"
            );
        }
        Verb::List { script } => {
            for name in secrets::list(&bucket, &script).await? {
                out.row(&Name(name))?;
            }
            out.finish()?;
        }
        Verb::Delete { script, name } => {
            secrets::delete(&bucket, &script, &name).await?;
            note!("deleted secret {name} of {script}");
        }
    }
    Ok(())
}

struct KeyLine(String);

impl crate::cli_output::Record for KeyLine {
    fn json(&self) -> serde_json::Value {
        serde_json::json!({ "key": self.0 })
    }

    fn text(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(&self.0)
    }
}

struct Name(String);

impl crate::cli_output::Record for Name {
    fn json(&self) -> serde_json::Value {
        serde_json::json!({ "name": self.0 })
    }

    fn text(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(&self.0)
    }
}

pub fn print_help() {
    let text = format!(
        "celld secret — store the values of per-class secrets\n\n\
USAGE:\n  celld secret keygen\n  celld secret put SCRIPT NAME [--value V | --file PATH | --pipe] [OPTIONS]\n  \
celld secret list SCRIPT [OPTIONS]\n  celld secret delete SCRIPT NAME [OPTIONS]\n\n\
A Durable Object class sees a secret only when the config's `celld` block lists\n\
it: \"celld\": {{ \"durable_objects\": {{ \"Class\": {{ \"secrets\": [\"NAME\"] }} }} }}.\n\
The values are stored in the fleet bucket under secrets/SCRIPT/NAME, sealed with\n\
AES-256-GCM under a key that is not in the bucket, and are never in a deployment.\n\
SCRIPT is the Worker's config `name`. A node opens the values when it loads a\n\
deployment, so a changed value takes effect at the next load (celld deploy, or a\n\
node restart). `put` reads the value from --value, --file, or standard input.\n\n\
OPTIONS:\n{FLEET_HELP}\n  --secrets-key-file PATH  The key for `put` (default: CELLD_SECRETS_KEY_FILE or CELLD_SECRETS_KEY)\n  --json                   Print one JSON object per line\n\n\
`celld secret keygen` prints a fresh key. Give nodes and `put` the same key; keep it\n\
out of the bucket."
    );
    let _ = Output::new(Format::Text).help(&text);
}
