use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use servant::prelude::*;

use super::{NewTodo, Todo, UpdateTodo};

#[derive(Default)]
pub(super) struct Store {
    next_id: u64,
    todos: BTreeMap<u64, Todo>,
}

pub(super) type SharedStore = Arc<Mutex<Store>>;

pub(super) fn new_store() -> SharedStore {
    Arc::new(Mutex::new(Store::default()))
}

pub(super) fn list_todos(
    store: SharedStore,
) -> impl Fn() -> std::future::Ready<Result<Vec<Todo>, ServerError>> + Clone + Send + Sync + 'static
{
    move || {
        let result =
            lock_store(&store).map(|store| store.todos.values().cloned().collect::<Vec<_>>());
        std::future::ready(result)
    }
}

pub(super) fn create_todo(
    store: SharedStore,
) -> impl Fn(NewTodo) -> std::future::Ready<Result<Todo, ServerError>> + Clone + Send + Sync + 'static
{
    move |new| {
        let result = lock_store(&store).map(|mut store| {
            store.next_id += 1;
            let todo = Todo {
                id: store.next_id,
                title: new.title,
                completed: false,
            };
            store.todos.insert(todo.id, todo.clone());
            todo
        });
        std::future::ready(result)
    }
}

pub(super) fn get_todo(
    store: SharedStore,
) -> impl Fn(u64) -> std::future::Ready<Result<Todo, ServerError>> + Clone + Send + Sync + 'static {
    move |id| {
        let result = lock_store(&store).and_then(|store| {
            store
                .todos
                .get(&id)
                .cloned()
                .ok_or_else(|| ServerError::err404().with_body("todo not found"))
        });
        std::future::ready(result)
    }
}

pub(super) fn update_todo(
    store: SharedStore,
) -> impl Fn(u64, UpdateTodo) -> std::future::Ready<Result<Todo, ServerError>>
+ Clone
+ Send
+ Sync
+ 'static {
    move |id, update| {
        let result = lock_store(&store).and_then(|mut store| {
            store
                .todos
                .get_mut(&id)
                .ok_or_else(|| ServerError::err404().with_body("todo not found"))
                .map(|todo| {
                    if let Some(title) = update.title {
                        todo.title = title;
                    }
                    if let Some(completed) = update.completed {
                        todo.completed = completed;
                    }
                    todo.clone()
                })
        });
        std::future::ready(result)
    }
}

pub(super) fn delete_todo(
    store: SharedStore,
) -> impl Fn(u64) -> std::future::Ready<Result<NoContent, ServerError>> + Clone + Send + Sync + 'static
{
    move |id| {
        let result = lock_store(&store).and_then(|mut store| match store.todos.remove(&id) {
            Some(_) => Ok(NoContent),
            None => Err(ServerError::err404().with_body("todo not found")),
        });
        std::future::ready(result)
    }
}

fn lock_store(store: &SharedStore) -> Result<MutexGuard<'_, Store>, ServerError> {
    store
        .lock()
        .map_err(|_| ServerError::err500().with_body("todo store lock poisoned"))
}
