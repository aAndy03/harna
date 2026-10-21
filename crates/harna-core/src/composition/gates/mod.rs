//! Group `gates`: the verdicts — preflight (release gate),
//! validate-intent (claims vs state), doctor (installation health),
//! score (the health grade and its trend).

pub mod doctor;
pub mod finish;
pub mod intent;
pub mod preflight;
pub mod score;
