// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Queries the exclusive systems make every frame, kept from one frame to the next. `World::query` builds a new query
//! state each time it is called, matching it against every archetype in the world (hundreds here); a kept one only
//! looks at the archetypes made since it last ran. In the browser's profile the fresh ones cost about a twentieth of
//! the main thread's frame.
use std::any::{Any, TypeId};

use bevy::{
    ecs::query::{QueryData, QueryFilter},
    platform::collections::HashMap,
    prelude::*,
};

/// The kept query states, by their type.
#[derive(Resource, Default)]
pub struct Queries(HashMap<TypeId, Box<dyn Any + Send + Sync>>);

/// Runs `f` with this world's kept query state for `D` and `F` (made the first time).
pub fn with<D: QueryData + 'static, F: QueryFilter + 'static, R>(world: &mut World, f: impl FnOnce(&mut World, &mut QueryState<D, F>) -> R) -> R {
    let key = TypeId::of::<QueryState<D, F>>();
    let kept = world.get_resource_mut::<Queries>().and_then(|mut queries| queries.0.remove(&key));
    let mut state = match kept.and_then(|state| state.downcast::<QueryState<D, F>>().ok()) {
        Some(state) => *state,
        None => QueryState::new(world),
    };
    let out = f(world, &mut state);
    world.get_resource_or_insert_with(Queries::default).0.insert(key, Box::new(state));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Component)]
    struct A(u32);
    #[derive(Component)]
    struct B;

    #[test]
    fn a_kept_query_sees_what_was_spawned_after_it_was_made() {
        let mut world = World::new();
        world.spawn(A(1));
        let count = |world: &mut World| with::<&A, (), _>(world, |world, q| q.iter(world).map(|a| a.0).sum::<u32>());
        assert_eq!(count(&mut world), 1);
        // A new archetype, and more of the old one.
        world.spawn((A(10), B));
        world.spawn(A(100));
        assert_eq!(count(&mut world), 111);
        assert_eq!(with::<&A, With<B>, _>(&mut world, |world, q| q.iter(world).count()), 1);
        // Mutable, and kept per type.
        with::<&mut A, Without<B>, _>(&mut world, |world, q| q.iter_mut(world).for_each(|mut a| a.0 += 1));
        assert_eq!(count(&mut world), 113);
        assert_eq!(world.resource::<Queries>().0.len(), 3);
    }
}
