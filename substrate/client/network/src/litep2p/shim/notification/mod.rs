// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! Shim for `litep2p::NotificationHandle` to combine `Peerset`-like behavior
//! with `NotificationService`.

use crate::{
	error::Error,
	litep2p::shim::notification::peerset::{Peerset, PeersetNotificationCommand},
	service::{
		metrics::Metrics,
		traits::{Direction, NotificationEvent as SubstrateNotificationEvent, ValidationResult},
	},
	MessageSink, NotificationService, ProtocolName,
};

use futures::{future::BoxFuture, stream::FuturesUnordered, StreamExt};
use litep2p::protocol::notification::{
	self, NotificationError, NotificationEvent, NotificationHandle, NotificationSink,
};
use tokio::sync::oneshot;

use sc_network_types::PeerId;

use std::fmt;

pub mod config;
pub mod peerset;

// TODO: add lots of tests

/// Logging target for the file.
const LOG_TARGET: &str = "sub-libp2p::notification";

/// Register opened substream to Prometheus.
fn register_substream_opened(metrics: &Option<Metrics>, protocol: &ProtocolName) {
	if let Some(metrics) = metrics {
		metrics.notifications_streams_opened_total.with_label_values(&[&protocol]).inc();
	}
}

/// Register closed substream to Prometheus.
fn register_substream_closed(metrics: &Option<Metrics>, protocol: &ProtocolName) {
	if let Some(metrics) = metrics {
		metrics
			.notifications_streams_closed_total
			.with_label_values(&[&protocol[..]])
			.inc();
	}
}

/// Register sent notification to Prometheus.
fn register_notification_sent(metrics: &Option<Metrics>, protocol: &ProtocolName, size: usize) {
	if let Some(metrics) = metrics {
		metrics
			.notifications_sizes
			.with_label_values(&["out", protocol])
			.observe(size as f64);
	}
}

/// Register received notification to Prometheus.
fn register_notification_received(metrics: &Option<Metrics>, protocol: &ProtocolName, size: usize) {
	if let Some(metrics) = metrics {
		metrics
			.notifications_sizes
			.with_label_values(&["in", protocol])
			.observe(size as f64);
	}
}

/// Wrapper over `litep2p`'s notification sink.
pub struct Litep2pMessageSink {
	/// Protocol.
	protocol: ProtocolName,

	/// Remote peer ID.
	peer: PeerId,

	/// Notification sink.
	sink: NotificationSink,

	/// Notification metrics.
	metrics: Option<Metrics>,
}

impl Litep2pMessageSink {
	/// Create new [`Litep2pMessageSink`].
	fn new(
		peer: PeerId,
		protocol: ProtocolName,
		sink: NotificationSink,
		metrics: Option<Metrics>,
	) -> Self {
		Self { protocol, peer, sink, metrics }
	}
}

#[async_trait::async_trait]
impl MessageSink for Litep2pMessageSink {
	/// Send synchronous `notification` to the peer associated with this [`MessageSink`].
	fn send_sync_notification(&self, notification: Vec<u8>) {
		let size = notification.len();

		match self.sink.send_sync_notification(notification) {
			Ok(_) => register_notification_sent(&self.metrics, &self.protocol, size),
			Err(error) => log::trace!(
				target: LOG_TARGET,
				"{}: failed to send sync notification to {:?}: {error:?}",
				self.protocol,
				self.peer,
			),
		}
	}

	/// Send an asynchronous `notification` to to the peer associated with this [`MessageSink`],
	/// allowing sender to exercise backpressure.
	///
	/// Returns an error if the peer does not exist.
	async fn send_async_notification(&self, notification: Vec<u8>) -> Result<(), Error> {
		let size = notification.len();

		match self.sink.send_async_notification(notification).await {
			Ok(_) => {
				register_notification_sent(&self.metrics, &self.protocol, size);
				Ok(())
			},
			Err(error) => {
				log::trace!(
					target: LOG_TARGET,
					"{}: failed to send async notification to {:?}: {error:?}",
					self.protocol,
					self.peer,
				);

				Err(Error::Litep2p(error))
			},
		}
	}
}

/// Notification protocol implementation.
pub struct NotificationProtocol {
	/// Protocol name.
	protocol: ProtocolName,

	/// `litep2p` notification handle.
	handle: NotificationHandle,

	/// Peerset for the notification protocol.
	///
	/// Listens to peering-related events and either opens or closes substreams to remote peers.
	peerset: Peerset,

	/// Pending validations for inbound substreams.
	pending_validations: FuturesUnordered<
		BoxFuture<'static, (PeerId, Result<ValidationResult, oneshot::error::RecvError>)>,
	>,

	/// Notification metrics.
	metrics: Option<Metrics>,
}

impl fmt::Debug for NotificationProtocol {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("NotificationProtocol")
			.field("protocol", &self.protocol)
			.field("handle", &self.handle)
			.field("peerset", &self.peerset)
			.finish()
	}
}

impl NotificationProtocol {
	/// Create new [`NotificationProtocol`].
	pub fn new(
		protocol: ProtocolName,
		handle: NotificationHandle,
		peerset: Peerset,
		metrics: Option<Metrics>,
	) -> Self {
		Self { protocol, handle, peerset, metrics, pending_validations: FuturesUnordered::new() }
	}

	/// Handle `Peerset` command.
	async fn on_peerset_command(&mut self, command: PeersetNotificationCommand) {
		match command {
			PeersetNotificationCommand::OpenSubstream { peers } => {
				if let Err(_) = tokio::time::timeout(
					std::time::Duration::from_secs(10),
					self.handle.open_substream_batch(peers.into_iter().map(From::from)),
				)
				.await
				{
					panic!("{}: open substreams, channel clogged", self.protocol);
				}
			},
			PeersetNotificationCommand::CloseSubstream { peers } => {
				if let Err(_) = tokio::time::timeout(
					std::time::Duration::from_secs(10),
					self.handle.close_substream_batch(peers.into_iter().map(From::from)),
				)
				.await
				{
					panic!("{}: close substreams, channel clogged", self.protocol);
				}
			},
		}
	}
}

#[async_trait::async_trait]
impl NotificationService for NotificationProtocol {
	async fn open_substream(&mut self, peer: PeerId) -> Result<(), ()> {
		unimplemented!();
	}

	async fn close_substream(&mut self, peer: PeerId) -> Result<(), ()> {
		unimplemented!();
	}

	fn send_sync_notification(&mut self, peer: &PeerId, notification: Vec<u8>) {
		let size = notification.len();

		if let Ok(_) = self.handle.send_sync_notification(peer.into(), notification) {
			register_notification_sent(&self.metrics, &self.protocol, size);
		}
	}

	async fn send_async_notification(
		&mut self,
		peer: &PeerId,
		notification: Vec<u8>,
	) -> Result<(), Error> {
		let size = notification.len();

		match self.handle.send_async_notification(peer.into(), notification).await {
			Ok(_) => {
				register_notification_sent(&self.metrics, &self.protocol, size);
				Ok(())
			},
			Err(_) => Err(Error::ChannelClosed),
		}
	}

	/// Set handshake for the notification protocol replacing the old handshake.
	async fn set_handshake(&mut self, handshake: Vec<u8>) -> Result<(), ()> {
		self.handle.set_handshake(handshake);

		Ok(())
	}

	/// Non-blocking variant of `set_handshake()` that attempts to update the handshake
	/// and returns an error if the channel is blocked.
	///
	/// Technically the function can return an error if the channel to `Notifications` is closed
	/// but that doesn't happen under normal operation.
	fn try_set_handshake(&mut self, handshake: Vec<u8>) -> Result<(), ()> {
		self.handle.set_handshake(handshake);

		Ok(())
	}

	/// Make a copy of the object so it can be shared between protocol components
	/// who wish to have access to the same underlying notification protocol.
	fn clone(&mut self) -> Result<Box<dyn NotificationService>, ()> {
		unimplemented!("clonable `NotificationService` not supported by `litep2p`");
	}

	/// Get protocol name of the `NotificationService`.
	fn protocol(&self) -> &ProtocolName {
		&self.protocol
	}

	/// Get message sink of the peer.
	fn message_sink(&self, peer: &PeerId) -> Option<Box<dyn MessageSink>> {
		self.handle.notification_sink(peer.into()).map(|sink| {
			let sink: Box<dyn MessageSink> = Box::new(Litep2pMessageSink::new(
				*peer,
				self.protocol.clone(),
				sink,
				self.metrics.clone(),
			));
			sink
		})
	}

	/// Get next event from the `Notifications` event stream.
	async fn next_event(&mut self) -> Option<SubstrateNotificationEvent> {
		loop {
			tokio::select! {
				event = self.handle.next() => match event? {
					NotificationEvent::ValidateSubstream {
						protocol,
						fallback,
						peer,
						handshake,
					} => {
						if std::matches!(self.peerset.report_inbound_substream(peer.into()), ValidationResult::Reject) {
							self.handle.send_validation_result(peer, notification::ValidationResult::Reject);
							continue;
						}

						let (tx, rx) = oneshot::channel();
						self.pending_validations.push(Box::pin(async move { (peer.into(), rx.await) }));

						log::trace!(target: LOG_TARGET, "{}: validate substream for {peer:?}", self.protocol);

						return Some(SubstrateNotificationEvent::ValidateInboundSubstream {
							peer: peer.into(),
							handshake,
							result_tx: tx,
						});
					}
					NotificationEvent::NotificationStreamOpened {
						peer,
						fallback,
						handshake,
						direction,
						..
					} => {
						// TODO: read return value here
						if !self.peerset.report_substream_opened(peer.into(), direction.into()) {
							if let Err(_) = tokio::time::timeout(
								std::time::Duration::from_secs(10),
								self.handle.close_substream_batch(vec![peer].into_iter().map(From::from)),
							)
							.await
							{
								panic!("{}: close substreams, channel clogged", self.protocol);
							}

							continue
						}

						log::trace!(target: LOG_TARGET, "{}: substream opened for {peer:?}", self.protocol);

						register_substream_opened(&self.metrics, &self.protocol);

						return Some(SubstrateNotificationEvent::NotificationStreamOpened {
							peer: peer.into(),
							handshake,
							direction: direction.into(),
							negotiated_fallback: fallback.map(|protocol| match protocol {
								litep2p::ProtocolName::Static(protocol) => ProtocolName::from(protocol),
								litep2p::ProtocolName::Allocated(protocol) => ProtocolName::from(protocol),
							}),
						});
					}
					NotificationEvent::NotificationStreamClosed {
						peer,
					} => {
						self.peerset.report_substream_closed(peer.into());
						register_substream_closed(&self.metrics, &self.protocol);
						log::trace!(target: LOG_TARGET, "{}: substream closed for {peer:?}", self.protocol);

						return Some(SubstrateNotificationEvent::NotificationStreamClosed { peer: peer.into() })
					}
					NotificationEvent::NotificationStreamOpenFailure {
						peer,
						error,
					} => {
						log::trace!(target: LOG_TARGET, "{}: open failure for {peer:?}", self.protocol);
						self.peerset.report_substream_open_failure(peer.into(), error);
					}
					NotificationEvent::NotificationReceived {
						peer,
						notification,
					} => {
						register_notification_received(&self.metrics, &self.protocol, notification.len());

						return Some(SubstrateNotificationEvent::NotificationReceived { peer: peer.into(), notification });
					}
				},
				result = self.pending_validations.next(), if !self.pending_validations.is_empty() => {
					let (peer, result) = result?;
					let validation_result = match result {
						Ok(ValidationResult::Accept) => notification::ValidationResult::Accept,
						_ => notification::ValidationResult::Reject
					};

					self.handle.send_validation_result(peer.into(), validation_result);
				}
				command = self.peerset.next() => self.on_peerset_command(command?).await,
			}
		}
	}
}
