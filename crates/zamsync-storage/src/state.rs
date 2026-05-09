use std::collections::HashMap;
use zamsync_core::{Event, SequenceNumber};

/// The state projection represents the current "view" of the world.
/// It is updated by applying events from the WAL.
pub trait StateStore {
    fn apply_event(&mut self, seq: SequenceNumber, event: &Event);
    fn last_applied_seq(&self) -> Option<SequenceNumber>;
}

#[derive(Debug, Default)]
pub struct Patient {
    pub id: String,
    pub name: String,
    pub age: u16,
    pub location: String,
}

#[derive(Debug, Default)]
pub struct MemoryStateStore {
    pub patients: HashMap<String, Patient>,
    pub inventory: HashMap<String, i32>,
    pub last_seq: Option<SequenceNumber>,
}

impl StateStore for MemoryStateStore {
    fn apply_event(&mut self, seq: SequenceNumber, event: &Event) {
        match event {
            Event::UpsertPatient { id, name, age, location } => {
                self.patients.insert(id.clone(), Patient {
                    id: id.clone(),
                    name: name.clone(),
                    age: *age,
                    location: location.clone(),
                });
            }
            Event::RecordObservation { .. } => {
                // For now, we don't store historical observations in this projection
                // but we could add a list per patient.
            }
            Event::UpdateInventory { medication_id, delta } => {
                let count = self.inventory.entry(medication_id.clone()).or_insert(0);
                *count += delta;
            }
        }
        self.last_seq = Some(seq);
    }

    fn last_applied_seq(&self) -> Option<SequenceNumber> {
        self.last_seq
    }
}
