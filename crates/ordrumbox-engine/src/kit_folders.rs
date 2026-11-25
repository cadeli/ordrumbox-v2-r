//! The kits as their folders make them: every folder of `kits/` is a kit and
//! every WAV file in it one of its samples; `kits.json` only adds what a file
//! cannot say (instrument, root note, decay, texts…) and the user's edits.

use std::collections::HashSet;

use ordrumbox_core::Issue;
use ordrumbox_core::instruments::{NOT_FOUND, catalog};

use crate::kit::{Kit, KitSample, KitsRead};

/// A folder of `kits/`: its name, the kit's, and its WAV files as sample
/// paths (`808_like/kick.wav`), in the order they are listed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KitFolder {
    pub name: String,
    pub paths: Vec<String>,
}

/// The instrument a file name gives (`findInstrumentFromFileName(name).id`),
/// `NOT_FOUND` when it gives none.
#[must_use]
pub fn key_from_file_name(file_name: &str) -> String {
    catalog().from_file_name(file_name).map_or_else(|| NOT_FOUND.to_owned(), |i| i.id.clone())
}

impl KitSample {
    /// A sample nothing describes: named after its file, its instrument
    /// guessed from that name, every setting at its default.
    #[must_use]
    pub fn from_file(path: &str) -> Self {
        let file_name = path.rsplit_once('/').map_or(path, |(_, name)| name);
        Self {
            key: key_from_file_name(file_name),
            display_name: file_name.to_owned(),
            path: path.to_owned(),
            ..Self::default()
        }
    }
}

/// The kits: those `described` (the shipped `kits.json`, each kit of the
/// user's `edited` one in the place of the shipped kit of its name, then the
/// user's other kits), less the samples whose file is gone (`exists`), then
/// the files of each folder that no kit lists nor removed, added to the kit
/// of the folder (made when no kit has its name). A file added that way takes
/// its shipped description when it has one. A kit left with no sample and no
/// folder goes. What is left out goes to the issues.
pub fn kits_from_folders(
    folders: &[KitFolder],
    shipped: &[Kit],
    edited: &[Kit],
    exists: impl Fn(&str) -> bool,
) -> KitsRead {
    let mut kits: Vec<Kit> =
        shipped.iter().map(|kit| edited.iter().find(|e| e.name == kit.name).unwrap_or(kit).clone()).collect();
    kits.extend(edited.iter().filter(|e| shipped.iter().all(|kit| kit.name != e.name)).cloned());
    let listed: HashSet<String> = kits
        .iter()
        .flat_map(|kit| kit.samples.iter().map(|s| s.path.clone()).chain(kit.removed.iter().cloned()))
        .collect();
    let mut issues = Vec::new();
    for kit in &mut kits {
        kit.samples.retain(|sample| {
            let found = exists(&sample.path);
            if !found {
                issues.push(Issue::new(&kit.name, format!("{}: no such file, left out", sample.path)));
            }
            found
        });
    }
    for folder in folders {
        let found = folder.paths.iter().filter(|path| !listed.contains(*path)).map(|path| {
            shipped
                .iter()
                .flat_map(|kit| &kit.samples)
                .find(|s| &s.path == path)
                .cloned()
                .unwrap_or_else(|| KitSample::from_file(path))
        });
        match kits.iter_mut().find(|kit| kit.name == folder.name) {
            Some(kit) => kit.samples.extend(found),
            None => kits.push(Kit { name: folder.name.clone(), samples: found.collect(), ..Kit::default() }),
        }
    }
    kits.retain(|kit| {
        let kept = !kit.samples.is_empty() || folders.iter().any(|f| f.name == kit.name);
        if !kept {
            issues.push(Issue::new(&kit.name, "no folder and no sample: left out"));
        }
        kept
    });
    (kits, issues)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str, files: &[&str]) -> KitFolder {
        KitFolder { name: name.into(), paths: files.iter().map(|f| format!("{name}/{f}")).collect() }
    }

    fn sample(path: &str, key: &str) -> KitSample {
        KitSample {
            key: key.into(),
            display_name: key.into(),
            path: path.into(),
            decay_ms: Some(5000.0),
            ..KitSample::default()
        }
    }

    fn kit(name: &str, samples: Vec<KitSample>) -> Kit {
        Kit { name: name.into(), samples, ..Kit::default() }
    }

    fn paths(kit: &Kit) -> Vec<&str> {
        kit.samples.iter().map(|s| s.path.as_str()).collect()
    }

    #[test]
    fn every_folder_is_a_kit_and_every_file_a_sample() {
        let folders = [folder("a", &["kick.wav", "snare.wav"]), folder("b", &["hihat.wav"])];
        let (kits, issues) = kits_from_folders(&folders, &[], &[], |_| true);
        assert!(issues.is_empty());
        let names: Vec<_> = kits.iter().map(|k| k.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
        assert_eq!(paths(&kits[0]), ["a/kick.wav", "a/snare.wav"]);
        assert_eq!((kits[0].samples[0].key.as_str(), kits[0].samples[0].display_name.as_str()), ("KICK", "kick.wav"));
        assert_eq!(kits[0].samples[0].decay_ms, None);
    }

    #[test]
    fn kits_json_describes_and_orders_what_the_folders_hold() {
        let folders = [folder("z", &["a.wav", "kick.wav", "new.wav"]), folder("y", &["x.wav"])];
        let shipped = [
            kit("z", vec![sample("z/kick.wav", "KICK"), sample("z/a.wav", "SNARE")]),
            kit("gone", vec![sample("gone/kick.wav", "KICK")]),
        ];
        let (kits, issues) = kits_from_folders(&folders, &shipped, &[], |path| !path.starts_with("gone/"));
        let names: Vec<_> = kits.iter().map(|k| k.name.as_str()).collect();
        assert_eq!(names, ["z", "y"]);
        assert_eq!(paths(&kits[0]), ["z/kick.wav", "z/a.wav", "z/new.wav"]);
        assert_eq!((kits[0].samples[1].key.as_str(), kits[0].samples[1].decay_ms), ("SNARE", Some(5000.0)));
        let told: Vec<_> = issues.iter().map(|i| format!("{}: {}", i.at, i.message)).collect();
        assert_eq!(told, ["gone: gone/kick.wav: no such file, left out", "gone: no folder and no sample: left out"]);
    }

    #[test]
    fn the_users_kits_win_and_keep_their_moves_and_removals() {
        let folders = [folder("a", &["kick.wav", "snare.wav", "tom.wav", "new.wav"]), folder("b", &["clap.wav"])];
        let shipped = [
            kit("a", vec![sample("a/kick.wav", "KICK"), sample("a/snare.wav", "SNARE"), sample("a/tom.wav", "TOM")]),
            kit("b", vec![sample("b/clap.wav", "CLAP")]),
        ];
        let mut tuned = sample("a/kick.wav", "KICK");
        tuned.gain_db = -6.0;
        let edited = [
            Kit { removed: vec!["a/tom.wav".into()], ..kit("a", vec![tuned]) },
            kit("b", vec![sample("b/clap.wav", "CLAP"), sample("a/snare.wav", "SNARE")]),
            kit("mine", vec![sample("imported/mine/x.wav", "CHH")]),
        ];
        let (kits, issues) = kits_from_folders(&folders, &shipped, &edited, |_| true);
        assert!(issues.is_empty());
        assert_eq!(paths(&kits[0]), ["a/kick.wav", "a/new.wav"], "moved and removed samples stay out");
        assert_eq!(kits[0].samples[0].gain_db, -6.0);
        assert_eq!(paths(&kits[1]), ["b/clap.wav", "a/snare.wav"]);
        assert_eq!(kits[2].name, "mine");
    }

    #[test]
    fn a_folder_emptied_by_the_user_stays_a_kit() {
        let folders = [folder("a", &["kick.wav"])];
        let edited = [Kit { removed: vec!["a/kick.wav".into()], ..kit("a", Vec::new()) }];
        let (kits, issues) = kits_from_folders(&folders, &[], &edited, |_| true);
        assert!(issues.is_empty());
        assert_eq!((kits.len(), kits[0].samples.len()), (1, 0));
    }
}
