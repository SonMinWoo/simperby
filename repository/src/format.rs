use crate::{raw::RawCommit, raw::SemanticCommit, UNKNOWN_COMMIT_AUTHOR};
use eyre::{eyre, Error};
use regex::Regex;
use simperby_core::{reserved::ReservedState, *};

/// Converts a commit to a semantic commit.
pub fn to_semantic_commit(
    commit: &Commit,
    mut reserved_state: ReservedState,
) -> Result<SemanticCommit, Error> {
    match commit {
        Commit::Agenda(agenda) => {
            let title = format!(">agenda: {}", agenda.height);
            let body = serde_spb::to_string(agenda).unwrap();
            Ok(SemanticCommit {
                title,
                body,
                diff: Diff::None,
                author: agenda.author.clone(),
                timestamp: agenda.timestamp,
            })
        }
        Commit::Block(block_header) => {
            let title = format!(">block: {}", block_header.height);
            let body = serde_spb::to_string(block_header).unwrap();
            Ok(SemanticCommit {
                title,
                body,
                diff: Diff::None,
                author: if block_header.author == PublicKey::zero() {
                    "genesis".to_owned()
                } else {
                    reserved_state
                        .query_name(&block_header.author)
                        .ok_or_else(|| {
                            eyre!(
                                "failed to query the name of the author: {}",
                                block_header.author
                            )
                        })?
                },
                timestamp: block_header.timestamp,
            })
        }
        Commit::Transaction(transaction) => Ok(SemanticCommit {
            title: transaction.head.clone(),
            body: transaction.body.clone(),
            diff: transaction.diff.clone(),
            author: transaction.author.clone(),
            timestamp: transaction.timestamp,
        }),
        Commit::AgendaProof(agenda_proof) => {
            let title = format!(">agenda-proof: {}", agenda_proof.height);
            let body = serde_spb::to_string(agenda_proof).unwrap();
            Ok(SemanticCommit {
                title,
                body,
                diff: Diff::None,
                author: UNKNOWN_COMMIT_AUTHOR.to_owned(),
                timestamp: agenda_proof.timestamp,
            })
        }
        Commit::ExtraAgendaTransaction(tx) => {
            let body = serde_spb::to_string(tx).unwrap();
            match tx {
                ExtraAgendaTransaction::Delegate(tx) => {
                    let title = format!(
                        ">tx-delegate: {} to {}",
                        tx.data.delegator, tx.data.delegatee
                    );
                    let diff = Diff::Reserved(Box::new(reserved_state.apply_delegate(tx).unwrap()));
                    Ok(SemanticCommit {
                        title,
                        body,
                        diff,
                        author: tx.data.delegator.clone(),
                        timestamp: tx.data.timestamp,
                    })
                }
                ExtraAgendaTransaction::Undelegate(tx) => {
                    let title = format!(">tx-undelegate: {}", tx.data.delegator);
                    let diff =
                        Diff::Reserved(Box::new(reserved_state.apply_undelegate(tx).unwrap()));
                    Ok(SemanticCommit {
                        title,
                        body,
                        diff,
                        author: tx.data.delegator.clone(),
                        timestamp: tx.data.timestamp,
                    })
                }
                ExtraAgendaTransaction::Report(_) => {
                    unimplemented!("report is not implemented yet.")
                }
            }
        }
        Commit::ChatLog(_) => unimplemented!(),
    }
}

/// Converts a semantic commit to a commit.
///
/// TODO: retrieve author and timestamp from the commit metadata.
pub fn from_semantic_commit(semantic_commit: SemanticCommit) -> Result<Commit, Error> {
    let pattern = Regex::new(
        r"^>(((agenda)|(block)|(agenda-proof)): (\d+))|((tx-delegate): ((\D+)-(\d+)) to ((\D+)-(\d+)))|((tx-undelegate): ((\D+)-(\d+)))$"
    )
    .unwrap();
    let captures = pattern.captures(&semantic_commit.title);
    if let Some(captures) = captures {
        let commit_type = captures
            .get(2)
            .or_else(|| captures.get(8))
            .or_else(|| captures.get(16))
            .map(|m| m.as_str())
            .ok_or_else(|| {
                eyre!(
                    "failed to parse commit type from the commit title: {}",
                    semantic_commit.title
                )
            })?;
        match commit_type {
            "agenda" => {
                let agenda: Agenda = serde_spb::from_str(&semantic_commit.body)?;
                let height = captures.get(6).map(|m| m.as_str()).ok_or_else(|| {
                    eyre!(
                        "failed to parse height from the commit title: {}",
                        semantic_commit.title
                    )
                })?;
                let height = height.parse::<u64>()?;
                if height != agenda.height {
                    return Err(eyre!(
                        "agenda height mismatch: expected {}, got {}",
                        agenda.height,
                        height
                    ));
                }
                Ok(Commit::Agenda(agenda))
            }
            "block" => {
                let block_header: BlockHeader = serde_spb::from_str(&semantic_commit.body)?;
                let height = captures.get(6).map(|m| m.as_str()).ok_or_else(|| {
                    eyre!(
                        "failed to parse height from the commit title: {}",
                        semantic_commit.title
                    )
                })?;
                let height = height.parse::<u64>()?;
                if height != block_header.height {
                    return Err(eyre!(
                        "block height mismatch: expected {}, got {}",
                        block_header.height,
                        height
                    ));
                }
                Ok(Commit::Block(block_header))
            }
            "agenda-proof" => {
                let agenda_proof: AgendaProof = serde_spb::from_str(&semantic_commit.body)?;
                let height = captures.get(6).map(|m| m.as_str()).ok_or_else(|| {
                    eyre!(
                        "failed to parse height from the commit title: {}",
                        semantic_commit.title
                    )
                })?;
                let height = height.parse::<u64>()?;
                if height != agenda_proof.height {
                    return Err(eyre!(
                        "agenda-proof height mismatch: expected {}, got {}",
                        agenda_proof.height,
                        height
                    ));
                }
                Ok(Commit::AgendaProof(agenda_proof))
            }
            "tx-delegate" => {
                let tx: ExtraAgendaTransaction = serde_spb::from_str(&semantic_commit.body)?;
                match tx {
                    ExtraAgendaTransaction::Delegate(ref tx) => {
                        let delegator = captures.get(9).map(|m| m.as_str()).ok_or_else(|| {
                            eyre!(
                                "failed to parse delegator from the commit title: {}",
                                semantic_commit.title
                            )
                        })?;
                        if delegator != tx.data.delegator {
                            return Err(eyre!(
                                "delegator mismatch: expected {}, got {}",
                                delegator,
                                tx.data.delegator
                            ));
                        }
                        let delegatee = captures.get(12).map(|m| m.as_str()).ok_or_else(|| {
                            eyre!(
                                "failed to parse delegatee from the commit title: {}",
                                semantic_commit.title
                            )
                        })?;
                        if delegatee != tx.data.delegatee {
                            return Err(eyre!(
                                "delegatee mismatch: expected {}, got {}",
                                delegatee,
                                tx.data.delegatee
                            ));
                        }
                        Ok(Commit::ExtraAgendaTransaction(
                            ExtraAgendaTransaction::Delegate(tx.clone()),
                        ))
                    }
                    _ => Err(eyre!("expected delegation transaction, got {:?}", tx)),
                }
            }
            "tx-undelegate" => {
                let tx: ExtraAgendaTransaction = serde_spb::from_str(&semantic_commit.body)?;
                match tx {
                    ExtraAgendaTransaction::Undelegate(ref tx) => {
                        let delegator = captures.get(17).map(|m| m.as_str()).ok_or_else(|| {
                            eyre!(
                                "failed to parse delegator from the commit title: {}",
                                semantic_commit.title
                            )
                        })?;
                        if delegator != tx.data.delegator {
                            return Err(eyre!(
                                "delegator mismatch: expected {}, got {}",
                                delegator,
                                tx.data.delegator
                            ));
                        }
                        Ok(Commit::ExtraAgendaTransaction(
                            ExtraAgendaTransaction::Undelegate(tx.clone()),
                        ))
                    }
                    _ => Err(eyre!("expected undelegation transaction, got {:?}", tx)),
                }
            }
            _ => Err(eyre!("unknown commit type: {}", commit_type)),
        }
    } else {
        Ok(Commit::Transaction(Transaction {
            author: semantic_commit.author,
            timestamp: semantic_commit.timestamp,
            head: semantic_commit.title,
            body: semantic_commit.body,
            diff: semantic_commit.diff,
        }))
    }
}

pub fn fp_to_semantic_commit(fp: &LastFinalizationProof) -> SemanticCommit {
    let title = format!(">fp: {}", fp.height);
    let body = serde_spb::to_string(&fp).unwrap();
    SemanticCommit {
        title,
        body,
        diff: Diff::None,
        author: UNKNOWN_COMMIT_AUTHOR.to_owned(),
        timestamp: 0,
    }
}

pub fn fp_from_semantic_commit(
    semantic_commit: SemanticCommit,
) -> Result<LastFinalizationProof, Error> {
    let pattern = Regex::new(r"^>fp: (\d+)$").unwrap();
    let captures = pattern.captures(&semantic_commit.title);
    if let Some(captures) = captures {
        let height = captures.get(1).map(|m| m.as_str()).ok_or_else(|| {
            eyre!(
                "Failed to parse commit height from commit title: {}",
                semantic_commit.title
            )
        })?;
        let height = height.parse::<u64>()?;
        let proof: LastFinalizationProof = serde_spb::from_str(&semantic_commit.body)?;
        if height != proof.height {
            return Err(eyre!(
                "proof height mismatch: expected {}, got {}",
                proof.height,
                height
            ));
        }
        Ok(proof)
    } else {
        Err(eyre!("unknown commit type: {}", semantic_commit.title))
    }
}

pub fn raw_commit_to_semantic_commit(raw_commit: RawCommit) -> SemanticCommit {
    let (title, body) = if let Some((title, body)) = raw_commit.message.split_once("\n\n") {
        (title.to_string(), body.to_string())
    } else {
        (String::new(), String::new())
    };
    SemanticCommit {
        title,
        body,
        diff: if raw_commit.diff.is_none() {
            Diff::None
        } else {
            // TODO: should handle cases, `Reserved`, `NonReserved, `General`.
            unimplemented!()
        },
        author: raw_commit.author,
        timestamp: raw_commit.timestamp,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use simperby_core::test_utils::generate_standard_genesis;

    #[test]
    fn format_transaction_commit() {
        let (reserved_state, _) = generate_standard_genesis(4);
        let transaction = Commit::Transaction(Transaction {
            author: "doesn't matter".to_owned(),
            timestamp: 0,
            head: "abc".to_string(),
            body: "def".to_string(),
            diff: Diff::None,
        });
        assert_eq!(
            transaction,
            from_semantic_commit(to_semantic_commit(&transaction, reserved_state).unwrap(),)
                .unwrap()
        );
    }

    #[test]
    fn format_agenda_commit() {
        let (reserved_state, _) = generate_standard_genesis(4);
        let agenda = Commit::Agenda(Agenda {
            height: 3,
            author: "doesn't matter".to_owned(),
            timestamp: 123,
            transactions_hash: Hash256::hash("hello"),
            previous_block_hash: Hash256::hash("hello"),
        });
        assert_eq!(
            agenda,
            from_semantic_commit(to_semantic_commit(&agenda, reserved_state).unwrap(),).unwrap()
        );
    }

    #[test]
    fn format_block_commit() {
        let (reserved_state, _) = generate_standard_genesis(4);
        let block = Commit::Block(BlockHeader {
            height: 3,
            author: PublicKey::zero(),
            prev_block_finalization_proof: FinalizationProof {
                round: 0,
                signatures: vec![TypedSignature::new(Signature::zero(), PublicKey::zero())],
            },
            previous_hash: Hash256::hash("hello1"),
            timestamp: 0,
            commit_merkle_root: Hash256::hash("hello2"),
            repository_merkle_root: Hash256::hash("hello3"),
            validator_set: vec![(PublicKey::zero(), 1)],
            version: SIMPERBY_CORE_PROTOCOL_VERSION.to_string(),
        });
        assert_eq!(
            block,
            from_semantic_commit(to_semantic_commit(&block, reserved_state).unwrap(),).unwrap()
        );
    }

    #[test]
    fn format_agenda_proof_commit() {
        let (reserved_state, _) = generate_standard_genesis(4);
        let agenda_proof = Commit::AgendaProof(AgendaProof {
            height: 3,
            agenda_hash: Hash256::hash("hello1"),
            proof: vec![TypedSignature::new(Signature::zero(), PublicKey::zero())],
            timestamp: 0,
        });
        assert_eq!(
            agenda_proof,
            from_semantic_commit(to_semantic_commit(&agenda_proof, reserved_state).unwrap(),)
                .unwrap()
        );
    }

    #[test]
    fn format_extra_agenda_transaction_commit1() {
        let (reserved_state, keys) = generate_standard_genesis(4);
        let delegation_transaction_data = DelegationTransactionData {
            delegator: reserved_state.members[0].name.clone(),
            delegatee: reserved_state.members[1].name.clone(),
            governance: true,
            block_height: 0,
            timestamp: 0,
            chain_name: reserved_state.genesis_info.chain_name.clone(),
        };
        let delegation_transaction =
            Commit::ExtraAgendaTransaction(ExtraAgendaTransaction::Delegate(TxDelegate {
                data: delegation_transaction_data.clone(),
                proof: TypedSignature::sign(&delegation_transaction_data, &keys[0].1).unwrap(),
            }));
        assert_eq!(
            delegation_transaction,
            from_semantic_commit(
                to_semantic_commit(&delegation_transaction, reserved_state).unwrap()
            )
            .unwrap()
        );
    }

    #[test]
    fn format_extra_agenda_transaction_commit2() {
        let (mut reserved_state, keys) = generate_standard_genesis(4);
        reserved_state.members[0].governance_delegatee = Option::from("member-0000".to_string());
        reserved_state.members[0].consensus_delegatee = Option::from("member-0000".to_string());
        let undelegation_transaction_data = UndelegationTransactionData {
            delegator: reserved_state.members[0].name.clone(),
            block_height: 0,
            timestamp: 0,
            chain_name: reserved_state.genesis_info.chain_name.clone(),
        };
        let undelegation_transaction =
            Commit::ExtraAgendaTransaction(ExtraAgendaTransaction::Undelegate(TxUndelegate {
                data: undelegation_transaction_data.clone(),
                proof: TypedSignature::sign(&undelegation_transaction_data, &keys[0].1).unwrap(),
            }));
        assert_eq!(
            undelegation_transaction,
            from_semantic_commit(
                to_semantic_commit(&undelegation_transaction, reserved_state.clone()).unwrap()
            )
            .unwrap()
        );
    }

    #[test]
    fn format_fp() {
        let fp = LastFinalizationProof {
            height: 3,
            proof: FinalizationProof {
                round: 0,
                signatures: vec![
                    TypedSignature::new(Signature::zero(), PublicKey::zero()),
                    TypedSignature::new(Signature::zero(), PublicKey::zero()),
                ],
            },
        };
        assert_eq!(
            fp,
            fp_from_semantic_commit(fp_to_semantic_commit(&fp)).unwrap()
        );
    }
}
