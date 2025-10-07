//! Iterator wrapper around [`VTPushParser`].

use crate::{
    VTPushParser,
    event::{VTEvent, VTOwnedEvent},
};

/// A convenience wrapper around [`VTPushParser`] that implements [`Iterator`]
/// for any `Iterator` of `AsRef<[u8]>`.
///
/// _PERFORMANCE NOTE_: This will allocate significantly more than using the
/// parser directly, but may be more convenient for some use cases.
pub struct VTIterator<I>
where
    I: Iterator,
    I::Item: AsRef<[u8]>,
{
    parser: VTPushParser,
    iter: I,
    events: Vec<VTOwnedEvent>,
}

impl<I> VTIterator<I>
where
    I: Iterator,
    I::Item: AsRef<[u8]>,
{
    pub fn new(iter: I) -> Self {
        Self {
            parser: VTPushParser::new(),
            iter,
            events: Vec::new(),
        }
    }
}

impl<I> Iterator for VTIterator<I>
where
    I: Iterator,
    I::Item: AsRef<[u8]>,
{
    type Item = VTOwnedEvent;

    fn next(&mut self) -> Option<Self::Item> {
        while self.events.is_empty() {
            if let Some(next) = self.iter.next() {
                self.parser.feed_with(next.as_ref(), &mut |event: VTEvent| {
                    self.events.push(event.to_owned());
                });
            } else {
                return None;
            }
        }

        self.events.pop()
    }
}
