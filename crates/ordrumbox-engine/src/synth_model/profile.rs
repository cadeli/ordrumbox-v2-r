//! A preset's editor profile (`profile`): percussion, melodic or bass. It
//! changes no sound: it says which cards the editor shows first, the others
//! waiting in its advanced view.

use ordrumbox_core::json::{Object, Value};

/// The key of a preset that holds its profile.
pub const PROFILE_KEY: &str = "profile";
/// The profiles, as a preset writes them, and their labels; a preset without one shows "All".
pub const PROFILES: [(&str, &str); 3] = [("percussion", "Perc"), ("melodic", "Melodic"), ("bass", "Bass")];
/// The label of no profile: every card of before, the new ones in the advanced view.
pub const NO_PROFILE_LABEL: &str = "All";

/// The two views of the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CardView {
    /// The cards of the preset's profile.
    #[default]
    Main,
    /// The other cards.
    Advanced,
}

/// The cards added with the profiles: without one, only the advanced view shows them.
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

/// The view that shows card `group` of `draft`.
#[must_use]
pub fn card_view(draft: &Object, group: &str) -> CardView {
    let is_main = match profile(draft) {
        None => !LATER_CARDS.contains(&group),
        Some("percussion") => PERCUSSION_CARDS.contains(&group),
        Some("melodic") => MELODIC_CARDS.contains(&group),
        Some(_) => BASS_CARDS.contains(&group),
    };
    if is_main { CardView::Main } else { CardView::Advanced }
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
    fn each_card_is_in_one_view_and_a_profile_moves_them() {
        let mut d = defaults();
        let main = |d: &Object| -> Vec<String> {
            groups(d).into_iter().filter(|g| card_view(d, g) == CardView::Main).collect()
        };
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
