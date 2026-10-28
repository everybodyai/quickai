//! The first page of the recommended list (docs/catalog.md, section 1): what a person wants a model for, and the
//! few models of the list that do all of it, chosen for the memory of this computer. A person may choose several
//! tasks; the models shown do every one, and there may be none. Pure: the same entries, tasks and memory always give
//! the same picks.

use crate::heuristics::{fit, Fit};

use super::present::{Entry, Offer};
use super::types::TaskDef;

/// The most models the page shows besides its three picks.
pub const MAX_ALSO: usize = 3;

/// The tasks some entry does, in the order of `tasks`: a task no model of the engine does has no button.
pub fn offered<'a>(tasks: &'a [TaskDef], entries: &[Entry]) -> Vec<&'a TaskDef> {
    tasks
        .iter()
        .filter(|t| {
            entries
                .iter()
                .any(|e| e.card.as_ref().is_some_and(|c| c.tasks.contains(&t.id)))
        })
        .collect()
}

/// The places of the entries that do every task of `chosen`, in their order; none when nothing is chosen.
pub fn able(entries: &[Entry], chosen: &[String]) -> Vec<usize> {
    if chosen.is_empty() {
        return Vec::new();
    }
    entries
        .iter()
        .enumerate()
        .filter(|(_, e)| super::view::does_all(e, chosen))
        .map(|(i, _)| i)
        .collect()
}

/// The offer a single button fetches: the one the list recommends, with what goes with it when the entry has it
/// that way (an entry of the list puts those first), else the first.
pub fn main_offer(entry: &Entry) -> Option<&Offer> {
    entry
        .offers
        .iter()
        .find(|o| o.recommended)
        .or_else(|| entry.offers.first())
}

/// What the page shows for the tasks chosen, as places in the entries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Picks {
    /// The one to get: the largest of the models to start with that fits with room to spare, else the largest
    /// that does, else the smallest of all.
    pub best: Option<usize>,
    /// `best` fits with room to spare (else nothing that does the tasks does).
    pub best_fits: bool,
    /// One about half as big as `best`: faster, and lighter on the computer.
    pub smaller: Option<usize>,
    /// The smallest, when it is smaller still.
    pub smallest: Option<usize>,
    /// The other models to start with that do it and fit, smallest first.
    pub also: Vec<usize>,
    /// How many entries do every task chosen.
    pub able: usize,
}

/// The picks for `chosen` among `entries`, for a computer of `ram` bytes of memory (0: not known, everything fits).
/// The models of the type that is the own of a task chosen (`TaskDef::home`) are picked from first: a person who asks
/// for Chat is shown assistants before the code models that also chat.
pub fn pick(entries: &[Entry], chosen: &[String], tasks: &[TaskDef], ram: u64) -> Picks {
    let able = able(entries, chosen);
    if able.is_empty() {
        return Picks::default();
    }
    let homes: Vec<&str> = chosen
        .iter()
        .filter_map(|c| tasks.iter().find(|t| &t.id == c))
        .filter_map(TaskDef::home)
        .collect();
    let at_home: Vec<usize> = able
        .iter()
        .copied()
        .filter(|&i| {
            entries[i]
                .model_type
                .as_deref()
                .is_some_and(|k| homes.contains(&k))
        })
        .collect();
    let pool = if at_home.is_empty() {
        able.clone()
    } else {
        at_home
    };

    let size = |i: usize| main_offer(&entries[i]).and_then(|o| o.bytes).unwrap_or(0);
    let comfortable = |i: usize| fit(size(i), ram) == Fit::Comfortable;
    let start = |i: usize| entries[i].card.as_ref().is_some_and(|c| c.start);
    // ties go to the name, so that the same list always gives the same picks
    let key = |i: usize| (size(i), entries[i].name.to_lowercase());
    let largest = |of: &mut dyn Iterator<Item = usize>| of.max_by_key(|&i| key(i));
    let smallest_of = |of: &mut dyn Iterator<Item = usize>| of.min_by_key(|&i| key(i));
    // the models to start with among `of` when there are any, else all of `of`
    let start_first = |of: Vec<usize>| -> Vec<usize> {
        let started: Vec<usize> = of.iter().copied().filter(|&i| start(i)).collect();
        if started.is_empty() {
            of
        } else {
            started
        }
    };

    let fitting: Vec<usize> = pool.iter().copied().filter(|&i| comfortable(i)).collect();
    let (best, best_fits) = match largest(&mut start_first(fitting.clone()).into_iter()) {
        Some(i) => (i, true),
        None => match smallest_of(&mut pool.iter().copied()) {
            Some(i) => (i, false),
            None => return Picks::default(),
        },
    };

    let below = |than: usize| -> Vec<usize> {
        pool.iter()
            .copied()
            .filter(|&i| key(i) < key(than))
            .collect()
    };
    let half = size(best) / 2;
    let smaller = start_first(below(best))
        .into_iter()
        .min_by_key(|&i| (size(i).abs_diff(half), key(i)));
    let smallest = smaller.and_then(|s| smallest_of(&mut start_first(below(s)).into_iter()));

    let picked = [Some(best), smaller, smallest];
    let mut also: Vec<usize> = pool
        .iter()
        .copied()
        .filter(|&i| start(i) && comfortable(i) && !picked.contains(&Some(i)))
        .collect();
    also.sort_by_key(|&i| key(i));
    also.truncate(MAX_ALSO);

    Picks {
        best: Some(best),
        best_fits,
        smaller,
        smallest,
        also,
        able: able.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lookup::present::{Card, Fetch};

    const GB: u64 = 1_000_000_000;

    fn task(id: &str, types: &[&str], input: &[&str], features: &[&str]) -> TaskDef {
        let words = |list: &[&str]| list.iter().map(|w| w.to_string()).collect();
        TaskDef {
            id: id.to_string(),
            label: id.to_string(),
            help: String::new(),
            order: 0,
            types: words(types),
            input: words(input),
            features: words(features),
        }
    }

    fn tasks() -> Vec<TaskDef> {
        vec![
            task("chat", &["chat", "reasoning", "vision", "code"], &[], &[]),
            task("code", &["code"], &[], &[]),
            task("reason", &["reasoning"], &[], &["thinking"]),
            task("pictures", &["vision"], &["image"], &[]),
            task("transcribe", &["audio"], &["audio"], &[]),
        ]
    }

    /// A model of the list of `kind`, `gb` big, that does `does`.
    fn entry(name: &str, kind: &str, gb: f64, start: bool, does: &[&str]) -> Entry {
        let bytes = (gb * GB as f64) as u64;
        Entry {
            key: format!("catalog:{name}"),
            name: name.to_string(),
            description: String::new(),
            downloads: None,
            likes: None,
            tags: Vec::new(),
            model_type: Some(kind.to_string()),
            offers: vec![
                Offer {
                    bundle: None,
                    label: "small".to_string(),
                    hover: String::new(),
                    bytes: Some(bytes / 2),
                    estimated: false,
                    class: None,
                    recommended: false,
                    fetch: Fetch::Reference(format!("{name}:small")),
                },
                Offer {
                    bundle: None,
                    label: "best".to_string(),
                    hover: String::new(),
                    bytes: Some(bytes),
                    estimated: false,
                    class: None,
                    recommended: true,
                    fetch: Fetch::Reference(format!("{name}:best")),
                },
            ],
            card: Some(Box::new(Card {
                tasks: does.iter().map(|t| t.to_string()).collect(),
                start,
                ..Card::default()
            })),
        }
    }

    fn chat_list() -> Vec<Entry> {
        vec![
            entry("Tiny", "chat", 0.2, false, &["chat"]),
            entry("Small", "chat", 0.7, true, &["chat"]),
            entry("Middle", "chat", 3.2, true, &["chat", "reason", "pictures"]),
            entry(
                "Hears",
                "chat",
                5.7,
                true,
                &["chat", "pictures", "transcribe"],
            ),
            entry("Large", "chat", 6.1, true, &["chat", "reason", "pictures"]),
            entry("Huge", "chat", 14.0, false, &["chat", "reason", "pictures"]),
            entry("Coder", "code", 4.4, true, &["chat", "code"]),
            entry("Thinker", "reasoning", 4.7, true, &["chat", "reason"]),
        ]
    }

    fn names(entries: &[Entry], places: impl IntoIterator<Item = usize>) -> Vec<String> {
        places
            .into_iter()
            .map(|i| entries[i].name.clone())
            .collect()
    }

    #[test]
    fn the_largest_model_to_start_with_that_fits_is_the_pick_with_one_half_its_size_and_the_smallest_after_it(
    ) {
        let entries = chat_list();
        let picks = pick(&entries, &["chat".to_string()], &tasks(), 32 * GB);
        assert_eq!(names(&entries, picks.best), ["Large"]);
        assert!(picks.best_fits);
        // half of 6.1 is 3.05: the one to start with nearest it
        assert_eq!(names(&entries, picks.smaller), ["Middle"]);
        // the smallest to start with, not the smallest of all
        assert_eq!(names(&entries, picks.smallest), ["Small"]);
        // the code and reasoning models chat too, but a person who asks for Chat is shown the assistants
        assert_eq!(names(&entries, picks.also.clone()), ["Hears"]);
        assert_eq!(picks.able, 8);
    }

    #[test]
    fn several_tasks_are_done_together_or_not_at_all() {
        let entries = chat_list();
        let chosen = ["chat".to_string(), "transcribe".to_string()];
        let picks = pick(&entries, &chosen, &tasks(), 32 * GB);
        assert_eq!(names(&entries, picks.best), ["Hears"]);
        assert_eq!((picks.smaller, picks.smallest, picks.able), (None, None, 1));

        let chosen = ["code".to_string(), "transcribe".to_string()];
        assert_eq!(pick(&entries, &chosen, &tasks(), 32 * GB), Picks::default());
        assert_eq!(pick(&entries, &[], &tasks(), 32 * GB), Picks::default());
    }

    #[test]
    fn a_task_whose_own_type_no_model_that_does_them_all_has_is_picked_from_all_of_them() {
        let entries = chat_list();
        // Chat and Write code: only the code model does both, and it is not of the type Chat
        let chosen = ["chat".to_string(), "code".to_string()];
        let picks = pick(&entries, &chosen, &tasks(), 32 * GB);
        assert_eq!(names(&entries, picks.best), ["Coder"]);
        // Solve problems is the reasoning models' own: the thinker comes first though the assistants think too
        let picks = pick(&entries, &["reason".to_string()], &tasks(), 32 * GB);
        assert_eq!(names(&entries, picks.best), ["Thinker"]);
    }

    #[test]
    fn a_small_computer_is_shown_what_fits_it_and_the_smallest_when_nothing_does() {
        let entries = chat_list();
        // 8 GB: 4 GB with room to spare, less the program's share
        let picks = pick(&entries, &["chat".to_string()], &tasks(), 8 * GB);
        assert_eq!(names(&entries, picks.best), ["Middle"]);
        assert!(picks.best_fits);
        let picks = pick(&entries, &["chat".to_string()], &tasks(), GB / 4);
        assert_eq!(names(&entries, picks.best), ["Tiny"]);
        assert!(!picks.best_fits);
        assert_eq!((picks.smaller, picks.smallest), (None, None));
        assert!(picks.also.is_empty());
        // a memory that is not known judges nothing
        let picks = pick(&entries, &["chat".to_string()], &tasks(), 0);
        assert_eq!(names(&entries, picks.best), ["Large"]);
    }

    #[test]
    fn a_task_has_a_button_only_when_a_model_does_it_and_the_button_fetches_what_the_list_recommends(
    ) {
        let entries = chat_list();
        let all = tasks();
        let shown: Vec<&str> = offered(&all, &entries)
            .iter()
            .map(|t| t.id.as_str())
            .collect();
        assert_eq!(shown, ["chat", "code", "reason", "pictures", "transcribe"]);
        let shown: Vec<&str> = offered(&all, &entries[..2])
            .iter()
            .map(|t| t.id.as_str())
            .collect();
        assert_eq!(shown, ["chat"]);
        assert_eq!(main_offer(&entries[0]).unwrap().label, "best");
    }
}
