//! A preset's editor profile (`profile`): percussion, melodic or bass. It
//! changes no sound: it says which cards the editor shows first, the others
//! after them.

use ordrumbox_core::json::{Object, Value};

/// The key of a preset that holds its profile.
pub const PROFILE_KEY: &str = "profile";
/// The profiles, as a preset writes them, and their labels; a preset without one shows "All".
pub const PROFILES: [(&str, &str); 3] = [("percussion", "Perc"), ("melodic", "Melodic"), ("bass", "Bass")];
/// The label of no profile: every card of before first, the new ones after.
pub const NO_PROFILE_LABEL: &str = "All";

/// The cards added with the profiles: without one, they come last.
const LATER_CARDS: [&str; 3] = ["voice", "filterEnv", "filter2"];

/// The cards each profile shows first (the scope goes with the oscillators).
const PERCUSSION_CARDS: [&str; 10] =
    ["scope", "vco1", "vco2", "vco3", "fm", "noise", "filter", "envelope", "pitchEnv", "master"];
const MELODIC_CARDS: [&str; 12] =
    ["scope", "vco1", "vco2", "vco3", "unison", "filter", "filterEnv", "envelope", "lfo", "lfo2", "matrix", "master"];
const BASS_CARDS: [&str; 9] = ["scope", "vco1", "vco2", "vco3", "voice", "filter", "filterEnv", "envelope", "master"];

/// The profile of `draft`, if it has a known one.
#[must_use]
pub fn profile(draft: &Object) -> Option<&'static str> {
    let name = draft.get(PROFILE_KEY).and_then(Value::as_str)?;
    PROFILES.iter().map(|(p, _)| *p).find(|p| *p == name)
}

/// Whether the editor shows card `group` of `draft` among the first, those of its profile.
#[must_use]
pub fn is_shown_first(draft: &Object, group: &str) -> bool {
    match profile(draft) {
        None => !LATER_CARDS.contains(&group),
        Some("percussion") => PERCUSSION_CARDS.contains(&group),
        Some("melodic") => MELODIC_CARDS.contains(&group),
        Some(_) => BASS_CARDS.contains(&group),
    }
}

/// `draft` set to `profile` (`None`, or one unknown: none); whether it changed.
pub fn set_profile(draft: &mut Object, profile: Option<&str>) -> bool {
    let known = profile.and_then(|name| PROFILES.iter().map(|(p, _)| *p).find(|p| *p == name));
    if self::profile(draft) == known && (known.is_some() || draft.get(PROFILE_KEY).is_none()) {
        return false;
    }
    match known {
        Some(name) => {
            draft.insert(PROFILE_KEY, Value::from(name));
        }
        None => {
            draft.remove(PROFILE_KEY);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::super::{defaults, groups};
    use super::*;

    #[test]
    fn a_profile_says_which_cards_come_first() {
        let mut d = defaults();
        let main = |d: &Object| -> Vec<String> { groups(d).into_iter().filter(|g| is_shown_first(d, g)).collect() };
        assert!(!main(&d).iter().any(|g| LATER_CARDS.contains(&g.as_str())));
        assert!(main(&d).iter().any(|g| g == "matrix"));
        assert!(set_profile(&mut d, Some("bass")));
        assert_eq!(main(&d), BASS_CARDS);
        assert!(!set_profile(&mut d, Some("bass")));
        assert!(set_profile(&mut d, Some("nope")), "an unknown profile is none");
        assert_eq!(d.get(PROFILE_KEY), None);
        assert!(!set_profile(&mut d, None));
    }
}
