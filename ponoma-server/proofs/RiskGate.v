(* RiskGate.v — machine-checked proof of the ponoma risk-gate invariants (IDEA.md §6.2).

   This is the formal twin of `paper::risk_check` (ponoma-server/src/paper.rs) and the
   executable model-check in `src/verify.rs`. We model the gate over rationals (Coq `Q`) and
   prove the safety properties as theorems. The Rust gate is a faithful re-statement of this
   `admit` function, so these proofs certify the decision logic the whole agentic layer trusts.

   Check with:  rocq compile RiskGate.v   (or: coqc RiskGate.v)
   CI-enforced via proofs/_CoqProject + the Makefile `verify` target + .github/workflows/ci.yml.

   Properties:
     P1 no-short:      a SELL of more shares than held is rejected (allow_short = false).
     P2 no-overdraft:  a BUY whose notional exceeds available cash is rejected.
     P3 size-cap:      an order whose notional exceeds frac*account_value (>0) is rejected.
     P4 positivity:    a non-positive order (shares<=0 OR price<=0) is rejected — the model now
                       carries the same guard as paper.rs (`shares <= 0.0 || price <= 0.0`), and
                       P4soundness shows the converse: anything ADMITTED has shares>0 and price>0.
     P5 monotone-size: stated on the REAL `admit` — if an order is size-rejected, any larger
                       order (more shares, same price) is rejected too. *)

From Stdlib Require Import QArith.
From Stdlib Require Import Qabs.
From Stdlib Require Import Bool.
Local Open Scope Q_scope.

Inductive Action := Buy | Sell.

Record Limits := {
  max_order_frac  : Q;   (* e.g. 1/4 *)
  allow_short     : bool;
  max_cash_frac   : Q    (* e.g. 1 *)
}.

(* The gate: returns true iff the order is ADMITTED. Mirrors risk_check in paper.rs, INCLUDING
   the positivity guard `shares <= 0 || price <= 0 => reject` that paper.rs checks first. *)
Definition admit (l:Limits) (a:Action)
                  (shares price account cash held : Q) : bool :=
  (* positivity guard: reject non-positive shares/price (paper.rs rejects these outright) *)
  if orb (Qle_bool shares 0) (Qle_bool price 0)
  then false
  else
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
  destruct (orb (Qle_bool shares 0) (Qle_bool price 0)) eqn:Hg.
  - reflexivity.                       (* positivity guard already rejected *)
  - destruct (andb (Qle_bool 0 account && negb (Qeq_bool account 0))
                   (negb (Qle_bool (shares*price) (max_order_frac l * account)))) eqn:Hsize.
    + reflexivity.                     (* size-cap already rejected *)
    + rewrite Hshort. simpl.
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
  destruct (orb (Qle_bool shares 0) (Qle_bool price 0)) eqn:Hg.
  - reflexivity.
  - destruct (andb (Qle_bool 0 account && negb (Qeq_bool account 0))
                   (negb (Qle_bool (shares*price) (max_order_frac l * account)))) eqn:Hsize.
    + reflexivity.
    + assert (Hle : Qle_bool (shares*price) (max_cash_frac l * cash) = false).
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
  destruct (orb (Qle_bool shares 0) (Qle_bool price 0)) eqn:Hg.
  - reflexivity.                       (* positivity guard rejected (a fortiori) *)
  - assert (H0 : Qle_bool 0 account = true) by (apply Qle_bool_iff; apply Qlt_le_weak; exact Hacc).
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

(* ---- P4: positivity guard (matches paper.rs `shares<=0 || price<=0 => reject`) ---- *)
Theorem p4_positivity_guard :
  forall l a shares price account cash held,
    (shares <= 0 \/ price <= 0) ->
    admit l a shares price account cash held = false.
Proof.
  intros l a shares price account cash held Hpos.
  unfold admit.
  assert (Hg : orb (Qle_bool shares 0) (Qle_bool price 0) = true).
  { apply orb_true_iff. destruct Hpos as [Hs | Hp].
    - left.  apply Qle_bool_iff. exact Hs.
    - right. apply Qle_bool_iff. exact Hp. }
  rewrite Hg. reflexivity.
Qed.

(* ---- P4 soundness: the converse — anything ADMITTED has shares>0 and price>0 ---- *)
Theorem p4_admitted_is_positive :
  forall l a shares price account cash held,
    admit l a shares price account cash held = true ->
    0 < shares /\ 0 < price.
Proof.
  intros l a shares price account cash held Hadm.
  unfold admit in Hadm.
  destruct (orb (Qle_bool shares 0) (Qle_bool price 0)) eqn:Hg.
  - discriminate Hadm.                 (* guard true would have given false *)
  - apply orb_false_iff in Hg. destruct Hg as [Hs Hp].
    split.
    + (* Qle_bool shares 0 = false ==> ~ shares <= 0 ==> 0 < shares *)
      apply Qnot_le_lt. intro Hle.
      assert (Qle_bool shares 0 = true) by (apply Qle_bool_iff; exact Hle).
      rewrite Hs in H; discriminate.
    + apply Qnot_le_lt. intro Hle.
      assert (Qle_bool price 0 = true) by (apply Qle_bool_iff; exact Hle).
      rewrite Hp in H; discriminate.
Qed.

(* ---- P5: monotone in size, on the REAL gate ---- *)
(* If an order of s1 shares is size-rejected, then any order of s2 >= s1 shares at the same
   (positive) price is rejected by `admit` too. This references the gate directly (reusing P3),
   so it certifies the actual decision logic — not just an abstract inequality. *)
Theorem p5_monotone_size :
  forall l a price account cash held s1 s2,
    0 < account ->
    0 < price ->
    s1 <= s2 ->
    max_order_frac l * account < s1 * price ->
    admit l a s2 price account cash held = false.
Proof.
  intros l a price account cash held s1 s2 Hacc Hprice Hle Hsmall.
  apply p3_size_cap.
  - exact Hacc.
  - apply Qlt_le_trans with (y := s1 * price).
    + exact Hsmall.
    + apply Qmult_le_compat_r.
      * exact Hle.
      * apply Qlt_le_weak. exact Hprice.
Qed.
