//! The licensing seam.
//!
//! **No vendor is named here, and no billing is implemented.** The brief is
//! explicit: a merchant-of-record handles the money and a licence service
//! handles the keys, and neither is chosen yet. So this defines the *shape* of
//! the question the app asks — "may this machine run the paid features?" — and
//! leaves exactly one place to answer it.
//!
//! ## What lives here rather than in the frontend
//!
//! Everything. The frontend receives a [`LicenceState`] and nothing else: no
//! key, no token, no endpoint, no fingerprint. A licence check that the UI can
//! see the inputs to is a licence check anyone can read out of the bundle, and
//! `src/` ships as readable JavaScript inside the installer.
//!
//! ## Offline first
//!
//! Sim rigs live in garages, on machines that are not always online, and a
//! product that refuses to start a race because a licence server is down has
//! failed at the only moment that matters. So the design is:
//!
//! * A verified licence is cached locally with an expiry well beyond the
//!   check interval.
//! * Being offline extends the cache rather than invalidating it.
//! * A licence that has genuinely expired degrades to [`Tier::Unlicensed`],
//!   which still runs — it just stops writing to games.
//!
//! Deciding *which* features that covers is a product decision, so [`Tier`]
//! names them and nothing enforces them yet.

pub use tp_model::{LicenceState, Tier};

/// The one thing a licence vendor has to implement.
///
/// Deliberately three methods. Anything a specific service needs beyond this —
/// machine limits, seat management, trial periods — is its own business and
/// stays behind this line, so swapping vendors touches one file.
pub trait LicenceProvider: Send + Sync {
    /// The current entitlement. Must not block on the network: callers include
    /// the app's startup path.
    fn state(&self) -> LicenceState;

    /// Redeem a key the user has pasted in. This one may go to the network.
    fn activate(&self, key: &str) -> Result<LicenceState, String>;

    /// Release this machine's seat.
    fn deactivate(&self) -> Result<LicenceState, String>;
}

/// The provider used until a vendor is chosen: everything is licensed.
///
/// Not a stub that pretends to check. It grants the full tier and says so, so
/// that nothing in the app is written against a licence check that has never
/// returned false — the failure mode where enforcement is added at the end and
/// half the app turns out to assume it always passes.
pub struct Unrestricted;

impl LicenceProvider for Unrestricted {
    fn state(&self) -> LicenceState {
        LicenceState {
            tier: Tier::Licensed,
            reference: None,
            valid_until: None,
            offline: false,
            message: Some(
                "This build is unlicensed software in the literal sense: there \
                 is no licence check in it yet."
                    .into(),
            ),
        }
    }

    fn activate(&self, _key: &str) -> Result<LicenceState, String> {
        Err("This build has no licence service configured.".into())
    }

    fn deactivate(&self) -> Result<LicenceState, String> {
        Err("This build has no licence service configured.".into())
    }
}

/// The provider the app uses. One function to change when a vendor is picked.
pub fn provider() -> Box<dyn LicenceProvider> {
    Box::new(Unrestricted)
}

/// A stable, non-identifying id for this machine.
///
/// Licence services need to count machines. This gives them something to count
/// that is **not** a hardware serial, a MAC address, or anything else that
/// identifies the person or follows them to another product: a random value
/// generated once and kept in the app's own data directory.
///
/// It survives reinstalling the app only if the data directory survives, which
/// is the right trade — a fingerprint that outlives a clean reinstall is one
/// the user cannot clear.
pub fn machine_id() -> String {
    let path = crate::logging::app_data_dir().join("machine-id");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let fresh = uuid::Uuid::new_v4().to_string();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&path, &fresh) {
        // Not fatal. A machine id that cannot be stored means the next start
        // gets a new one, which costs a seat rather than breaking the app.
        tracing::warn!(error = %e, "could not store the machine id");
    }
    fresh
}

/// The last four characters of a key, for display.
///
/// Never the key. The point is telling two licences apart, which four
/// characters do and a whole key does not do any better.
pub fn reference_for(key: &str) -> String {
    let tail: String = key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reference_is_four_characters_and_never_the_key() {
        let key = "TP-ABCD-EFGH-IJKL-MNOP";
        let reference = reference_for(key);
        assert_eq!(reference, "…MNOP");
        assert!(!key.contains(&reference));
        // Separators do not leak into the reference and are not counted.
        assert_eq!(reference_for("A-B-C-D-E"), "…BCDE");
    }

    #[test]
    fn a_short_key_does_not_panic() {
        assert_eq!(reference_for("AB"), "…AB");
        assert_eq!(reference_for(""), "…");
    }

    #[test]
    fn the_placeholder_provider_grants_rather_than_pretends_to_check() {
        // If it returned Unlicensed, nothing would exercise the licensed path;
        // if it silently returned Licensed with no explanation, nobody would
        // notice there is no check.
        let state = Unrestricted.state();
        assert_eq!(state.tier, Tier::Licensed);
        assert!(state.message.is_some());
    }
}
