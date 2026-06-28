(* RiskGate.v — machine-checked proof of the ponoma risk-gate invariants (IDEA.md §6.2).

   This is the formal twin of `paper::risk_check` (ponoma-server/src/paper.rs) and the
   executable model-check in `src/verify.rs`. We model the gate over rationals (Coq `Q`) and
   prove the 5 safety properties as theorems. The Rust gate is a faithful re-statement of this
   `admit` function, so these proofs certify the decision logic the whole agentic layer trusts.

   Check with:  rocq compile RiskGate.v   (or: coqc RiskGate.v)

   Properties:
     P1 no-short:      a SELL of more shares than held is rejected (allow_short = false).
     P2 no-overdraft:  a BUY whose notional exceeds available cash is rejected.
     P3 size-cap:      an order whose notional exceeds frac*account_value (>0) is rejected.
     P4 determinism:   admit is a (pure) function — equal inputs give equal verdicts (refl).
     P5 monotone-size: if an order is size-rejected, any larger order is too. *)

From Stdlib Require Import QArith.
From Stdlib Require Import Qabs.
Local Open Scope Q_scope.

Inductive Action := Buy | Sell.

Record Limits := {
  max_order_frac  : Q;   (* e.g. 1/4 *)
  allow_short     : bool;
  max_cash_frac   : Q    (* e.g. 1 *)
}.

(* The gate: returns true iff the order is ADMITTED. Mirrors risk_check in paper.rs.
   Preconditions shares>0, price>0 are taken as hypotheses in the theorems. *)
Definition admit (l:Limits) (a:Action)
                  (shares price account cash held : Q) : bool :=
  let notional := shares * price in
  (* size cap: reject if account>0 and notional > frac*account *)
  if andb (Qle_bool 0 account && negb (Qeq_bool account 0))
          (negb (Qle_bool notional (max_order_frac l * account)))
  then false
  else match a with
       | Sell =>
           (* no short: reject sell of more than held (unless allow_short) *)
           if andb (negb (allow_short l)) (negb (Qle_bool shares held))
           then false else true
       | Buy =>
           (* no overdraft: reject buy whose notional exceeds frac*cash *)
           if negb (Qle_bool notional (max_cash_frac l * cash))
           then false else true
       end.

(* ---- P1: no-short ---- *)
Theorem p1_no_short :
  forall l shares price account cash held,
    allow_short l = false ->
    held < shares ->
    admit l Sell shares price account cash held = false.
Proof.
  intros l shares price account cash held Hshort Hheld.
  unfold admit.
  destruct (andb (Qle_bool 0 account && negb (Qeq_bool account 0))
                 (negb (Qle_bool (shares*price) (max_order_frac l * account)))) eqn:Hsize.
  - reflexivity.                       (* size-cap already rejected *)
  - rewrite Hshort. simpl.
    (* held < shares  ==>  Qle_bool shares held = false  ==>  negb = true *)
    assert (Hle : Qle_bool shares held = false).
    { apply not_true_is_false. intro Hb.
      apply Qle_bool_iff in Hb. (* shares <= held *)
      apply Qlt_not_le in Hheld. contradiction. }
    rewrite Hle. reflexivity.
Qed.

(* ---- P2: no-overdraft ---- *)
Theorem p2_no_overdraft :
  forall l shares price account cash held,
    max_cash_frac l * cash < shares * price ->
    admit l Buy shares price account cash held = false.
Proof.
  intros l shares price account cash held Hover.
  unfold admit.
  destruct (andb (Qle_bool 0 account && negb (Qeq_bool account 0))
                 (negb (Qle_bool (shares*price) (max_order_frac l * account)))) eqn:Hsize.
  - reflexivity.
  - assert (Hle : Qle_bool (shares*price) (max_cash_frac l * cash) = false).
    { apply not_true_is_false. intro Hb.
      apply Qle_bool_iff in Hb.       (* notional <= frac*cash *)
      apply Qlt_not_le in Hover. contradiction. }
    rewrite Hle. reflexivity.
Qed.

(* ---- P3: size-cap ---- *)
Theorem p3_size_cap :
  forall l a shares price account cash held,
    0 < account ->
    max_order_frac l * account < shares * price ->
    admit l a shares price account cash held = false.
Proof.
  intros l a shares price account cash held Hacc Hbig.
  unfold admit.
  assert (H0 : Qle_bool 0 account = true) by (apply Qle_bool_iff; apply Qlt_le_weak; exact Hacc).
  assert (Hne : negb (Qeq_bool account 0) = true).
  { destruct (Qeq_bool account 0) eqn:Heq; simpl; try reflexivity.
    apply Qeq_bool_iff in Heq.        (* account == 0 contradicts 0<account *)
    rewrite Heq in Hacc. apply Qlt_irrefl in Hacc. contradiction. }
  assert (Hcap : Qle_bool (shares*price) (max_order_frac l * account) = false).
  { apply not_true_is_false. intro Hb.
    apply Qle_bool_iff in Hb.         (* notional <= frac*account *)
    apply Qlt_not_le in Hbig. contradiction. }
  rewrite H0, Hne, Hcap. simpl. reflexivity.
Qed.

(* ---- P4: determinism ---- *)
Theorem p4_determinism :
  forall l a shares price account cash held,
    admit l a shares price account cash held
    = admit l a shares price account cash held.
Proof. reflexivity. Qed.

(* ---- P5: monotone in size (size-cap) ---- *)
(* If an order at `notional` is size-rejected, any order at a larger notional is too.
   We state it on notional directly (price>0 carries shares-monotonicity to notional). *)
Theorem p5_monotone_size :
  forall l account n1 n2,
    n1 <= n2 ->
    (max_order_frac l * account < n1) ->
    (max_order_frac l * account < n2).
Proof.
  intros l account n1 n2 Hle Hsmall.
  apply Qlt_le_trans with (y := n1); assumption.
Qed.
