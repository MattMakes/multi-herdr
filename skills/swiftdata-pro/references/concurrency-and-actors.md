# Concurrency and Actors

## Isolation Model

- Use `mainContext` for UI-bound operations.
- Use dedicated isolation for background persistence work.
- Avoid mixing long-running write flows directly in UI contexts.

## Model Actors

`@ModelActor` helps create actor-isolated persistence services with mutually exclusive access.

Benefits:

- serialized access to model operations,
- safer background processing,
- reduced accidental context sharing.

Pattern (a `Trip` is a model, so the actor takes plain values, not a model instance):

```swift
@ModelActor
actor TripStore {
    func saveTrip(name: String, startDate: Date) throws {
        modelContext.insert(Trip(name: name, startDate: startDate))
        try modelContext.save()
    }
}
```

- Do not add `@concurrent` to persistence methods. An actor already runs its work off the main actor. Use `@concurrent` only for CPU-bound work that must leave the caller's actor.
- Under CloudKit, a background writer still follows the CloudKit rules in `references/cloudkit.md`: no `#Unique`, defaults or optionals on every property, and eventual consistency.

## Context Boundaries

- Do not pass mutable model instances loosely across isolation boundaries.
- Pass identifiers (`persistentModelID`) and refetch in the receiving context when needed.
- Keep context ownership explicit in service boundaries.

## Undo and Concurrency

- Automatic undo/redo integration is tied to main-context save flows.
- Background contexts are not a drop-in replacement for undo-enabled user editing.

## History with Concurrent Writers

- Set `modelContext.author` for different writers when useful.
- Filter fetched history by token and author to separate signal from noise.

## Primary Documentation

- https://developer.apple.com/documentation/swiftdata/concurrencysupport
- https://developer.apple.com/documentation/swiftdata/modelactor()
- https://developer.apple.com/documentation/swiftdata/modelactor
- https://developer.apple.com/documentation/swiftdata/modelexecutor
- https://developer.apple.com/documentation/swiftdata/defaultserialmodelexecutor
