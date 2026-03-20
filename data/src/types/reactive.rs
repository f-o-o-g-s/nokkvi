//! Simple thread-safe state containers for ViewModels
//!
//! These provide synchronized access to shared state in a way that's compatible
//! with Iced's Elm Architecture. Unlike MVVM reactive bindings, these are just
//! synchronized data holders - Iced handles reactivity through its message/update cycle.
//!
//! Uses `parking_lot::RwLock` for:
//! - No mutex poisoning (won't panic if a thread panics while holding lock)
//! - Better performance than std::sync::Mutex
//! - Read-write separation for concurrent read access

use std::sync::Arc;

use parking_lot::RwLock;

/// Thread-safe property wrapper for ViewModels.
/// Provides synchronized get/set without MVVM-style reactive subscriptions.
/// Iced's message-based architecture handles UI updates through the update() cycle.
#[derive(Clone)]
pub struct ReactiveProperty<T> {
    value: Arc<RwLock<T>>,
}

impl<T> ReactiveProperty<T>
where
    T: Clone + Send + Sync + 'static,
{
    /// Create a new property with initial value
    pub fn new(initial_value: T) -> Self {
        Self {
            value: Arc::new(RwLock::new(initial_value)),
        }
    }

    /// Get the current value (read lock)
    pub fn get(&self) -> T {
        self.value.read().clone()
    }

    /// Set a new value (write lock)
    pub fn set(&self, new_value: T) {
        *self.value.write() = new_value;
    }
}

/// Thread-safe collection property for lists of items
#[derive(Clone)]
pub struct ReactiveVecProperty<T> {
    items: Arc<RwLock<Vec<T>>>,
}

impl<T> ReactiveVecProperty<T>
where
    T: Clone + Send + Sync + 'static,
{
    /// Create a new vector property
    pub fn new() -> Self {
        Self {
            items: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Get all items (read lock)
    pub fn get(&self) -> Vec<T> {
        self.items.read().clone()
    }

    /// Set all items (replaces the entire collection, write lock)
    pub fn set(&self, new_items: Vec<T>) {
        *self.items.write() = new_items;
    }

    /// Get the length without cloning the entire vec (read lock)
    pub fn len(&self) -> usize {
        self.items.read().len()
    }

    /// Check if empty without cloning (read lock)
    pub fn is_empty(&self) -> bool {
        self.items.read().is_empty()
    }
}

impl<T> Default for ReactiveVecProperty<T>
where
    T: Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe integer property (common case)
pub type ReactiveInt = ReactiveProperty<i32>;
