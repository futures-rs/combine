use std::error::Error;

use futures::Stream;

mod value;
pub use value::*;

/// A publisher delivers elements to one or more Subscriber instances.
///
/// call receive to create new subscriber stream.
pub trait Publisher {
    type Output;
    type Failure: Error;
    type Stream: Stream<Item = Self::Output> + Drop;

    ///  Create new receiver stream for this publisher
    fn receive(&mut self) -> Self::Stream;
}
