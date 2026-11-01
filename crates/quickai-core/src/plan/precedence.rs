//! The person's own arguments and lines of environment win over QuickAI's (`[[launch.spawn.option]]`).
//!
//! A program given an option twice takes one of them, or warns, or both: llama-server says "argument '--reasoning'
//! specified multiple times" and takes the last, and one that reads an option from a variable as well lets its
//! command line win, so a line `LLAMA_ARG_CTX_SIZE=8192` did nothing while QuickAI passed `--ctx-size`. What the
//! person wrote is what they meant: QuickAI's option is left out, and the log says so. Only what QuickAI needs to
//! reach the program (an option marked `keep`) stays QuickAI's, and then the person's is the one left out.

use crate::doc::engine::OptionSpec;

use super::types::{EnvOp, Traced};

/// What the arguments and the variables are once the person's have won, and what the log says about it.
pub(super) struct Settled {
    pub args: Vec<Traced<String>>,
    pub env: Vec<EnvOp>,
    pub notes: Vec<String>,
}

/// Whether a word of a command line is an option: `-` or `--` and then a letter (`-1` is a value, `--` alone the
/// end of the options).
pub fn is_option(word: &str) -> bool {
    let rest = word.trim_start_matches('-');
    rest.len() < word.len() && word.len() - rest.len() <= 2 && rest.starts_with(|c: char| c.is_ascii_alphabetic())
}

/// The option a word names, by the name it is known by: `-c` and `--ctx-size=8192` are `--ctx-size` when the
/// document lists them together, and `--no-mmap` is `--mmap`.
pub fn option_key(word: &str, options: &[OptionSpec]) -> String {
    let name = word.split('=').next().unwrap_or(word);
    let listed = |n: &str| {
        options
            .iter()
            .find(|o| o.names.iter().any(|x| x == n))
            .map(|o| o.names[0].clone())
    };
    if let Some(key) = listed(name) {
        return key;
    }
    // a switch said the other way round is the same option
    for (negative, positive) in [("--no-", "--"), ("-no-", "-")] {
        if let Some(rest) = name.strip_prefix(negative) {
            let named = format!("{positive}{rest}");
            return listed(&named).unwrap_or(named);
        }
    }
    name.to_string()
}

/// A run of words: an option and its values, or words before any option.
struct Group {
    start: usize,
    end: usize,
    person: bool,
    /// The option, for a run that begins with one.
    key: Option<String>,
}

/// The words cut into runs. The values of an option are the words after it up to the next option, and only the
/// ones from the same side: what QuickAI passes and what the person wrote are never one run.
fn groups(args: &[Traced<String>], person: &[bool], options: &[OptionSpec]) -> Vec<Group> {
    let mut out: Vec<Group> = Vec::new();
    for (i, word) in args.iter().enumerate() {
        let option = is_option(&word.value);
        match out.last_mut() {
            Some(last) if !option && last.person == person[i] && last.end == i => last.end = i + 1,
            _ => out.push(Group {
                start: i,
                end: i + 1,
                person: person[i],
                key: option.then(|| option_key(&word.value, options)),
            }),
        }
    }
    out
}

fn text(args: &[Traced<String>], g: &Group) -> String {
    args[g.start..g.end]
        .iter()
        .map(|a| a.value.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Settles `args` (with `person[i]` saying whether word `i` is one the person wrote) and `env` (in which the
/// variables named in `person_env` are the person's lines) for the options `options` lists. Two names are one
/// variable on Windows (`windows`).
pub(super) fn settle(
    options: &[OptionSpec],
    args: Vec<Traced<String>>,
    person: &[bool],
    mut env: Vec<EnvOp>,
    person_env: &[String],
    windows: bool,
) -> Settled {
    let same_var = |a: &str, b: &str| {
        if windows {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    };
    let runs = groups(&args, person, options);
    let mut drop = vec![false; runs.len()];
    let mut drop_env: Vec<String> = Vec::new();
    let mut notes = Vec::new();
    for (q, ours) in runs.iter().enumerate() {
        let Some(key) = ours.key.as_ref().filter(|_| !ours.person) else {
            continue;
        };
        let spec = options.iter().find(|o| &o.names[0] == key);
        let keep = spec.is_some_and(|o| o.keep);
        let what = format!("{} ({})", text(&args, ours), args[ours.start].origin);
        // the person's words for it
        for (p, theirs) in runs.iter().enumerate() {
            if !theirs.person || theirs.key.as_ref() != Some(key) {
                continue;
            }
            if keep {
                drop[p] = true;
                notes.push(format!(
                    "Your {} is left out: QuickAI reaches the program through its own {what}.",
                    text(&args, theirs)
                ));
            } else if !drop[q] {
                drop[q] = true;
                notes.push(format!(
                    "Your {} takes the place of QuickAI's {what}.",
                    text(&args, theirs)
                ));
            }
        }
        // the person's line for its variable
        let Some(var) = spec.and_then(|o| o.env.as_deref()) else {
            continue;
        };
        if let Some(line) = person_env.iter().find(|n| same_var(n, var)) {
            if keep {
                drop_env.push(line.clone());
                notes.push(format!(
                    "Your line {line} is left out: QuickAI reaches the program through its own {what}."
                ));
            } else if !drop[q] {
                drop[q] = true;
                notes.push(format!(
                    "Your line {line} takes the place of QuickAI's {what}: the program would let the command line win."
                ));
            }
        }
    }
    env.retain(|op| !drop_env.iter().any(|n| same_var(n, &op.name)));
    let mut kept = Vec::with_capacity(args.len());
    let mut args = args.into_iter();
    for (g, dropped) in runs.iter().zip(&drop) {
        for word in args.by_ref().take(g.end - g.start) {
            if !dropped {
                kept.push(word);
            }
        }
    }
    Settled {
        args: kept,
        env,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::types::EnvMode;

    fn option(names: &[&str], env: Option<&str>, keep: bool) -> OptionSpec {
        OptionSpec {
            names: names.iter().map(|n| n.to_string()).collect(),
            env: env.map(str::to_string),
            keep,
        }
    }

    fn words(text: &str, origin: &str) -> Vec<Traced<String>> {
        text.split_whitespace()
            .map(|w| Traced {
                value: w.to_string(),
                origin: origin.to_string(),
            })
            .collect()
    }

    fn settled(ours: &str, theirs: &str, lines: &[(&str, &str)], options: &[OptionSpec]) -> Settled {
        let mut args = words(ours, "engine");
        let mut person = vec![false; args.len()];
        let mine = words(theirs, "field extra_args");
        person.extend(std::iter::repeat_n(true, mine.len()));
        args.extend(mine);
        let env = lines
            .iter()
            .map(|(n, v)| EnvOp {
                name: n.to_string(),
                mode: EnvMode::Set {
                    value: v.to_string(),
                },
                origin: "field extra_env".into(),
            })
            .collect();
        let names: Vec<String> = lines.iter().map(|(n, _)| n.to_string()).collect();
        settle(options, args, &person, env, &names, false)
    }

    fn values(s: &Settled) -> String {
        s.args
            .iter()
            .map(|a| a.value.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn words_that_are_options() {
        assert!(is_option("--ctx-size"));
        assert!(is_option("-c"));
        assert!(is_option("-ngl"));
        assert!(is_option("--temp=0.7"));
        assert!(!is_option("-1"));
        assert!(!is_option("-0.5"));
        assert!(!is_option("--"));
        assert!(!is_option("-"));
        assert!(!is_option("---x"));
        assert!(!is_option("off"));
    }

    #[test]
    fn the_persons_option_takes_the_place_of_quickais_by_any_name() {
        let options = [option(&["--reasoning", "-rea"], Some("LLAMA_ARG_REASONING"), false)];
        let s = settled("--model m.gguf --reasoning off --temp 0.7", "-rea on", &[], &options);
        assert_eq!(values(&s), "--model m.gguf --temp 0.7 -rea on");
        assert_eq!(s.notes.len(), 1);
        assert!(s.notes[0].contains("Your -rea on takes the place of QuickAI's --reasoning off (engine)"));
    }

    #[test]
    fn an_option_that_is_not_listed_is_matched_by_its_name_and_a_switch_by_its_other_side() {
        let s = settled("--embeddings --threads 4 --mmap", "--threads=8 --no-mmap", &[], &[]);
        assert_eq!(values(&s), "--embeddings --threads=8 --no-mmap");
        assert_eq!(s.notes.len(), 2);
    }

    #[test]
    fn a_line_of_environment_for_an_option_takes_the_place_of_quickais() {
        let options = [option(&["--ctx-size", "-c"], Some("LLAMA_ARG_CTX_SIZE"), false)];
        let s = settled("--ctx-size 4096 --port 8080", "", &[("LLAMA_ARG_CTX_SIZE", "8192")], &options);
        assert_eq!(values(&s), "--port 8080");
        assert_eq!(s.env.len(), 1, "the person's line stays");
        assert!(s.notes[0].contains("Your line LLAMA_ARG_CTX_SIZE takes the place of QuickAI's --ctx-size 4096"));
    }

    #[test]
    fn what_quickai_needs_to_reach_the_program_stays_quickais() {
        let options = [option(&["--port"], Some("LLAMA_ARG_PORT"), true)];
        let s = settled("--port 8080 --host 127.0.0.1", "--port 9000 --host 0.0.0.0", &[("LLAMA_ARG_PORT", "1")], &options);
        assert_eq!(values(&s), "--port 8080 --host 0.0.0.0");
        assert!(s.env.is_empty(), "the line for the port is left out");
        assert!(s.notes.iter().any(|n| n.contains("Your --port 9000 is left out")));
        assert!(s.notes.iter().any(|n| n.contains("Your line LLAMA_ARG_PORT is left out")));
        assert!(s.notes.iter().any(|n| n.contains("Your --host 0.0.0.0 takes the place")));
    }

    #[test]
    fn words_before_any_option_and_values_that_look_like_numbers_stay_with_their_side() {
        let s = settled("serve --rope-freq-base -1", "--rope-freq-base 0", &[], &[]);
        assert_eq!(values(&s), "serve --rope-freq-base 0");
    }

    #[test]
    fn nothing_the_person_did_not_name_is_touched() {
        let s = settled("--model m.gguf --ctx-size 4096", "--threads 8 --flash-attn on", &[], &[]);
        assert_eq!(values(&s), "--model m.gguf --ctx-size 4096 --threads 8 --flash-attn on");
        assert!(s.notes.is_empty());
    }
}
