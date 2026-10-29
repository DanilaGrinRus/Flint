// Copyright 2021 The Grin Developers
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Generic implementation of owner API functions

use strum::IntoEnumIterator;

use super::owner::tx_lock_outputs;
use crate::api_impl::owner::{check_ttl, post_tx};
use crate::api_impl::types::update_tx_slate_state;
use crate::backend::WalletBackend;
use crate::grin_core::core::FeeFields;
use crate::grin_keychain::{Identifier, Keychain};
use crate::grin_util::secp::key::SecretKey;
use crate::internal::{selection, tx, updater};
use crate::slate_versions::{SlateVersion, VersionedSlate};
use crate::{
	address, BlockFees, CbData, Error, NodeClient, ReceiveRecovery, Slate, SlateState,
	TxLogEntryType, VersionInfo, RECEIVE_RECOVERY_VERSION,
};

const FOREIGN_API_VERSION: u16 = 2;

#[cfg(debug_assertions)]
#[derive(Clone, Copy, Debug, PartialEq)]
/// Test-only interruption points around the atomic receive commit.
pub enum ReceiveFailpoint {
	/// Fail before any transaction data is committed.
	BeforeCommit,
	/// Simulate a lost response after the commit succeeds.
	AfterCommit,
}

#[cfg(debug_assertions)]
thread_local! {
	static RECEIVE_FAILPOINT: std::cell::RefCell<Option<ReceiveFailpoint>> =
		const { std::cell::RefCell::new(None) };
}

#[cfg(debug_assertions)]
/// Arm a one-shot receive failpoint on the current thread.
pub fn set_receive_failpoint(failpoint: Option<ReceiveFailpoint>) {
	RECEIVE_FAILPOINT.with(|current| *current.borrow_mut() = failpoint);
}

#[cfg(debug_assertions)]
fn receive_failpoint(expected: ReceiveFailpoint) -> Result<(), Error> {
	let triggered = RECEIVE_FAILPOINT.with(|current| {
		if *current.borrow() == Some(expected) {
			*current.borrow_mut() = None;
			true
		} else {
			false
		}
	});
	if triggered {
		Err(Error::GenericError(
			"Тестовое прерывание receive_tx".to_owned(),
		))
	} else {
		Ok(())
	}
}

/// Return the version info
pub fn check_version() -> VersionInfo {
	VersionInfo {
		foreign_api_version: FOREIGN_API_VERSION,
		supported_slate_versions: SlateVersion::iter().collect(),
		receive_response_recovery: Some(RECEIVE_RECOVERY_VERSION as u16),
	}
}

/// Build a coinbase transaction
pub fn build_coinbase<C, K>(
	w: &mut WalletBackend<C, K>,
	keychain_mask: Option<&SecretKey>,
	block_fees: &BlockFees,
	test_mode: bool,
) -> Result<CbData, Error>
where
	C: NodeClient,
	K: Keychain,
{
	updater::build_coinbase(w, keychain_mask, block_fees, test_mode)
}

fn recover_receive_response<C, K>(
	w: &mut WalletBackend<C, K>,
	keychain_mask: Option<&SecretKey>,
	slate: &Slate,
	parent_key_id: &Identifier,
) -> Result<Option<Slate>, Error>
where
	C: NodeClient,
	K: Keychain,
{
	let txs = updater::retrieve_txs(w, None, Some(slate.id), None, Some(parent_key_id), false)?;
	if txs.iter().any(|t| {
		matches!(
			t.tx_type,
			TxLogEntryType::TxReceivedCancelled | TxLogEntryType::TxReverted
		)
	}) {
		return Err(Error::TransactionWasCancelled(slate.id.to_string()));
	}
	let received: Vec<_> = txs
		.iter()
		.filter(|t| t.tx_type == TxLogEntryType::TxReceived)
		.collect();
	let recovery = w.get_receive_recovery(parent_key_id, &slate.id)?;
	let recovery = match recovery {
		Some(r) => r,
		None if !received.is_empty() => {
			return Err(Error::ReceiveRecoveryUnavailable(slate.id.to_string()))
		}
		None => return Ok(None),
	};
	let request = VersionedSlate::into_version(slate.clone(), SlateVersion::V4)?;
	let request_matches = serde_json::to_vec(&request)
		.map_err(|_| Error::ReceiveRecoveryIntegrity("Ошибка сериализации запроса".to_owned()))?
		== serde_json::to_vec(&recovery.request)
			.map_err(|_| Error::ReceiveRecoveryIntegrity("Ошибка чтения запроса".to_owned()))?;
	if recovery.version != RECEIVE_RECOVERY_VERSION
		|| recovery.parent_key_id != *parent_key_id
		|| recovery.slate_id != slate.id
		|| !request_matches
		|| received.len() != 1
	{
		return Err(Error::ReceiveRecoveryIntegrity(slate.id.to_string()));
	}
	let entry = received[0];
	if entry.tx_slate_state != Some(SlateState::Standard2) {
		return Err(Error::ReceiveRecoveryIntegrity(slate.id.to_string()));
	}
	let outputs =
		updater::retrieve_outputs(w, keychain_mask, true, Some(entry.id), Some(parent_key_id))?;
	if outputs.len() != 1 || outputs[0].output.value != slate.amount {
		return Err(Error::ReceiveRecoveryIntegrity(slate.id.to_string()));
	}
	let response: Slate = recovery.response.into();
	if response.id != slate.id || response.state != SlateState::Standard2 {
		return Err(Error::ReceiveRecoveryIntegrity(slate.id.to_string()));
	}
	Ok(Some(response))
}

/// Receive a tx as recipient
pub fn receive_tx<C, K>(
	w: &mut WalletBackend<C, K>,
	keychain_mask: Option<&SecretKey>,
	slate: &Slate,
	dest_acct_name: Option<&str>,
	use_test_rng: bool,
) -> Result<Slate, Error>
where
	C: NodeClient,
	K: Keychain,
{
	if slate.amount == 0 {
		return Err(Error::InvalidAmount);
	}
	if slate.state != SlateState::Standard1 {
		return Err(Error::SlateState);
	}
	let parent_key_id = match dest_acct_name {
		Some(d) => {
			let pm = w.get_acct_path(d.to_owned())?;
			match pm {
				Some(p) => p.path,
				None => w.parent_key_id(),
			}
		}
		None => w.parent_key_id(),
	};
	if let Some(response) = recover_receive_response(w, keychain_mask, slate, &parent_key_id)? {
		return Ok(response);
	}
	check_ttl(w, slate)?;

	let mut ret_slate = slate.clone();
	ret_slate.tx = Some(Slate::empty_transaction());
	let height = w.last_confirmed_height_for_parent(&parent_key_id)?;
	let keychain = w.keychain(keychain_mask)?;
	let prepared = selection::prepare_recipient_output(
		w,
		keychain_mask,
		&mut ret_slate,
		height,
		parent_key_id.clone(),
		use_test_rng,
		false,
	)?;
	let mut context = prepared.context;
	let mut tx_log_entry = prepared.tx_log_entry;

	ret_slate.fill_round_1(&keychain, &mut context)?;
	context.initial_sec_key = context.sec_key.clone();
	ret_slate.fill_round_2(&keychain, &context.sec_key, &context.sec_nonce)?;
	tx_log_entry.kernel_excess = Some(ret_slate.calc_excess(keychain.secp())?);
	ret_slate.adjust_offset(&keychain, &context)?;

	let excess = ret_slate.calc_excess(keychain.secp())?;
	if let Some(ref mut p) = ret_slate.payment_proof {
		let sig = tx::create_payment_proof_signature(
			ret_slate.amount,
			&excess,
			p.sender_address,
			address::address_from_derivation_path(&keychain, &parent_key_id, 0)?,
		)?;
		p.receiver_signature = Some(sig);
	}

	ret_slate.amount = 0;
	ret_slate.fee_fields = FeeFields::zero();
	ret_slate.remove_other_sigdata(&keychain, &context.sec_nonce, &context.sec_key)?;
	ret_slate.state = SlateState::Standard2;
	tx_log_entry.tx_slate_state = Some(SlateState::Standard2);

	let recovery = ReceiveRecovery {
		version: RECEIVE_RECOVERY_VERSION,
		parent_key_id: parent_key_id.clone(),
		slate_id: slate.id,
		request: VersionedSlate::into_version(slate.clone(), SlateVersion::V4)?,
		response: VersionedSlate::into_version(ret_slate.clone(), SlateVersion::V4)?,
	};
	let mut batch = w.batch(keychain_mask)?;
	if batch.next_tx_log_id(&parent_key_id)? != tx_log_entry.id {
		return Err(Error::ReceiveRecoveryIntegrity(
			"Конфликт идентификатора транзакции".to_owned(),
		));
	}
	batch.save(prepared.output)?;
	batch.save_tx_log_entry(tx_log_entry, &parent_key_id)?;
	batch.save_receive_recovery(&recovery)?;
	#[cfg(debug_assertions)]
	receive_failpoint(ReceiveFailpoint::BeforeCommit)?;
	batch.commit()?;
	#[cfg(debug_assertions)]
	receive_failpoint(ReceiveFailpoint::AfterCommit)?;
	Ok(ret_slate)
}

/// Receive a tx that this wallet has issued
pub fn finalize_tx<C, K>(
	w: &mut WalletBackend<C, K>,
	keychain_mask: Option<&SecretKey>,
	slate: &Slate,
	post_automatically: bool,
) -> Result<Slate, Error>
where
	C: NodeClient,
	K: Keychain,
{
	let mut sl = slate.clone();
	let mut context = w.get_private_context(keychain_mask, sl.id.as_bytes())?;
	check_ttl(w, &sl)?;
	if sl.state == SlateState::Invoice2 {
		// Add our contribution to the offset
		sl.adjust_offset(&w.keychain(keychain_mask)?, &context)?;

		let mut temp_ctx = context.clone();
		temp_ctx.sec_key = context.initial_sec_key.clone();
		temp_ctx.sec_nonce = context.initial_sec_nonce.clone();
		selection::repopulate_tx(w, keychain_mask, &mut sl, &temp_ctx, false)?;

		tx::complete_tx(w, keychain_mask, &mut sl, &context)?;
		tx::update_stored_tx(w, keychain_mask, &context, &mut sl, true)?;
		{
			let mut batch = w.batch(keychain_mask)?;
			batch.delete_private_context(sl.id.as_bytes())?;
			batch.commit()?;
		}
		sl.state = SlateState::Invoice3;
		sl.amount = 0;

		let parent_key_id = w.parent_key_id();
		update_tx_slate_state(w, keychain_mask, &parent_key_id, &sl)?;
	} else if sl.state == SlateState::Standard2 {
		let keychain = w.keychain(keychain_mask)?;
		let parent_key_id = w.parent_key_id();

		if let Some(args) = context.late_lock_args.take() {
			// Transaction was late locked, select inputs+change now
			// and insert into original context

			let current_height = w.w2n_client().get_chain_tip()?.0;
			let mut temp_sl =
				tx::new_tx_slate(w, context.amount, false, 2, false, args.ttl_blocks)?;
			let temp_context = selection::build_send_tx(
				w,
				&keychain,
				keychain_mask,
				&mut temp_sl,
				current_height,
				args.minimum_confirmations,
				args.max_outputs as usize,
				args.num_change_outputs as usize,
				args.selection_strategy_is_use_all,
				Some(context.fee.map(|f| f.fee()).unwrap_or(0)),
				parent_key_id.clone(),
				false,
				true,
				false,
			)?;

			// Add inputs and outputs to original context
			context.input_ids = temp_context.input_ids;
			context.output_ids = temp_context.output_ids;

			// Store the updated context
			{
				let mut batch = w.batch(keychain_mask)?;
				batch.save_private_context(sl.id.as_bytes(), &context)?;
				batch.commit()?;
			}

			// Now do the actual locking
			tx_lock_outputs(w, keychain_mask, &sl)?;
		}

		// Add our contribution to the offset
		sl.adjust_offset(&keychain, &context)?;

		selection::repopulate_tx(w, keychain_mask, &mut sl, &context, true)?;

		tx::complete_tx(w, keychain_mask, &mut sl, &context)?;
		tx::verify_slate_payment_proof(w, keychain_mask, &parent_key_id, &context, &sl)?;
		tx::update_stored_tx(w, keychain_mask, &context, &sl, false)?;
		{
			let mut batch = w.batch(keychain_mask)?;
			batch.delete_private_context(sl.id.as_bytes())?;
			batch.commit()?;
		}
		sl.state = SlateState::Standard3;
		sl.amount = 0;

		update_tx_slate_state(w, keychain_mask, &parent_key_id, &sl)?;
	} else {
		return Err(Error::SlateState);
	}
	if post_automatically {
		post_tx(w.w2n_client(), sl.tx_or_err()?, true)?;
	}
	Ok(sl)
}
