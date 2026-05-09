use rkyv::{Archive, Deserialize, Serialize};
use bytecheck::CheckBytes;

#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
#[archive(check_bytes)]
pub enum Event {
    /// Create or update a patient record.
    UpsertPatient {
        id: String,
        name: String,
        age: u16,
        location: String,
    },
    /// Record clinical observation.
    RecordObservation {
        patient_id: String,
        observation_type: String, // e.g., "blood_pressure"
        value: String,
        unit: String,
    },
    /// Stock update for medication.
    UpdateInventory {
        medication_id: String,
        delta: i32,
    },
}
