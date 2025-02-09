use std::path::PathBuf;

use chrono::Local;
use gpui::{AppContext, Global};
use rusqlite::Connection;

use crate::{
    db::init_db,
    errors::Result,
    repositories::{flash_card::CardQueue, session::Session},
    storage::{self, sqlite::SqliteStorage},
    FlashCard,
};

use super::{
    answer::Answer, builder::Builder, card::get_current_card_state, queue::Queue,
    states::card_state::CardState, timing::SchedTimingToday,
};

pub struct CollectionBuilder {
    collection_path: Option<PathBuf>,
}

impl CollectionBuilder {
    pub fn new(col_path: PathBuf) -> Self {
        CollectionBuilder {
            collection_path: Some(col_path),
        }
    }

    pub(crate) fn timing_for_timestamp(conn: &Connection, now: i64) -> SchedTimingToday {
        // Get current utc offset from the system
        let creation_stamp_res = Session::get_creation_stamp(conn);

        let creation_stamp = if let Err(e) = creation_stamp_res {
            println!("Error getting creation stamp: {:?}", e);
            Session::set_creation_stamp(conn, now).unwrap();
            now
        } else {
            creation_stamp_res.unwrap()
        };

        let days_elapsed = (now - creation_stamp) / 86_400;
        let next_day_at = (days_elapsed + 1) * 86_400;

        SchedTimingToday {
            now,
            days_elapsed: days_elapsed as u32,
            next_day_at,
        }
    }
}

impl Builder for CollectionBuilder {
    type OutputType = Collection;

    fn build(&mut self) -> Result<Collection> {
        let col_path = self
            .collection_path
            .clone()
            .unwrap_or_else(|| PathBuf::from(":memory:"));

        println!("Opening collection at {:?}", col_path);

        let storage = SqliteStorage::open_or_create(&col_path)?;

        init_db(&storage.conn).unwrap_or_else(|e| {
            panic!("Error initializing database: {:?}", e);
        });

        let timing = Self::timing_for_timestamp(&storage.conn, Local::now().timestamp());

        let col = Collection {
            storage,
            col_path,
            card_queues: None,
            timing,
        };

        Ok(col)
    }
}

pub struct Collection {
    pub storage: SqliteStorage,
    pub col_path: PathBuf,
    pub card_queues: Option<Queue>,
    pub timing: SchedTimingToday,
}

impl Collection {
    pub fn init(col: Self, cx: &mut AppContext) {
        cx.set_global(col);
    }

    pub fn apply_state(&self, card: &mut FlashCard, next: CardState) {
        match next {
            CardState::New(next_new_state) => {
                card.due = next_new_state.position as u32;
                card.set_queue(CardQueue::New);
            }
            CardState::Learning(next_learning_state) => {
                card.set_queue(CardQueue::Learning);
                card.memory_state = next_learning_state.memory_state;
            }
            CardState::Review(next_review_state) => {
                card.set_queue(CardQueue::Review);
                card.interval = next_review_state.scheduled_days;
                card.due = (self.timing.days_elapsed + next_review_state.scheduled_days) as u32;
                card.memory_state = next_review_state.memory_state;
            }
            _ => {}
        }
    }

    pub fn answer_card(&self, card_id: u32, answer: Answer) -> FlashCard {
        let mut card = FlashCard::load(card_id, &self.storage.conn).unwrap();

        let current_card_state = get_current_card_state(&card);
        let next_state = current_card_state.next_states();

        match answer {
            Answer::Again => {
                let next = next_state.again;
                self.apply_state(&mut card, next);
            }
            Answer::Hard => {
                let next = next_state.hard;
                self.apply_state(&mut card, next);
            }
            Answer::Good => {
                let next = next_state.good;
                self.apply_state(&mut card, next);
            }
            Answer::Easy => {
                let next = next_state.easy;
                self.apply_state(&mut card, next);
            }
        }

        card.save(&self.storage.conn).unwrap();
        card
    }
}

impl Global for Collection {}

// impl Into<CollectionBuilder> for Collection {
//     fn into(self) -> CollectionBuilder {
//         CollectionBuilder {
//             collection_path: Some(self.col_path),
//             deck_id: None,
//         }
//     }
// }
