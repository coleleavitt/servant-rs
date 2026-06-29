# ponoma-server — formal-verification rail.
#
# `make verify` machine-checks the risk-gate proof (proofs/RiskGate.v, the formal twin of
# paper::risk_check) with Rocq/Coq. It is GUARDED: when neither `rocq` nor `coqc` is on PATH it
# prints a notice and exits 0, so the build never fails on a machine without the prover. CI runs
# this same target (see ../.github/workflows/ci.yml).
#
# The exhaustive executable model-check of the same invariants lives in src/verify.rs and runs
# under `cargo test` — the two rails are complementary.

PROOF := proofs/RiskGate.v

.PHONY: verify verify-clean test clippy

verify:
	@if command -v rocq >/dev/null 2>&1; then \
		echo "verify: rocq compile $(PROOF)"; \
		rocq compile $(PROOF); \
	elif command -v coqc >/dev/null 2>&1; then \
		echo "verify: coqc $(PROOF)"; \
		coqc $(PROOF); \
	else \
		echo "verify: SKIP — neither 'rocq' nor 'coqc' on PATH; install Rocq/Coq to machine-check $(PROOF)."; \
	fi

verify-clean:
	rm -f proofs/*.vo proofs/*.vok proofs/*.vos proofs/*.glob proofs/.*.aux

# Convenience wrappers for the Rust rails (run from the workspace so paths resolve).
test:
	cargo test -p ponoma-server

clippy:
	cargo clippy -p ponoma-server --all-targets -- -D warnings
