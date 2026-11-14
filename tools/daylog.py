#!/usr/bin/env python3
"""daylog.py LOG.jsonl [--max-moves 15] [--bid-gap 1200] [--roam-min 180] [--cover-max 12]
Audit a brain decision log (SA_BRAIN_LOG) against the hard rules (BRIEF-brain §4.3, §14).
Times are brain seconds. Forced decisions (`brain force …`) are exempt from the rate rules only.
Exit 1 if any hard rule is broken."""
import json, sys


def main():
    a = sys.argv[1:]
    path = a.pop(0)
    opt = {"--max-moves": 15, "--bid-gap": 1200.0, "--roam-min": 180.0, "--cover-max": 12.0}
    while a:
        k = a.pop(0)
        opt[k] = float(a.pop(0))
    rows = [json.loads(l) for l in open(path) if l.strip()]
    for r in rows:
        if r.get("k") == "start":
            s = r.get("settings", {})
            opt["--max-moves"] = s.get("max_moves_per_hour", opt["--max-moves"])
            opt["--bid-gap"] = s.get("bid_every_min", [20])[0] * 60
            opt["--roam-min"] = s.get("passive_every_min", [3])[0] * 60
            opt["--cover-max"] = s.get("cover_max_pct", opt["--cover-max"])
    bad = []
    forced = []  # brain times of forced decisions
    moves, arrivals, emotes = [], [], []
    fs_now = False
    for r in rows:
        k, t = r.get("k"), r.get("t", 0.0)
        if k == "brain" and r["what"].startswith("control Force"):
            forced.append((t, r["what"]))
        if k == "senses":
            fs_now = r["s"].get("fs", False)
        if k == "move":
            moves.append(r)
        if k == "arrived":
            arrivals.append(r)
        if k == "emote":
            emotes.append(r)

    def was_forced(t, what):
        return any(abs(t - ft) < 0.5 and what in w for ft, w in forced)

    # R1-R5: per move.
    for m in moves:
        t = m["t"]
        if m["fs"]:
            bad.append((t, "R1 moved while a fullscreen window was up", m["goal"]))
        if m["dnd"] or not m["enabled"]:
            bad.append((t, "R2 moved under DND/off", m["goal"]))
        if m["spot"] != "in_place":
            x, y, w, h = m["body"]
            c = m["clear_px"]
            cx, cy = m["cursor"]
            if x - c <= cx < x + w + c and y - c <= cy < y + h + c:
                bad.append((t, "R3 body within the cursor clearance", m["spot"]))
        if m["cover_pct"] > opt["--cover-max"] + 1e-6:
            bad.append((t, f"R4 covers {m['cover_pct']}% of his window", m["spot"]))
        if m["goal"] in ("passive", "passive_far", "leave") and m["cover_pct"] > 0:
            bad.append((t, "R5 a passive spot over his window", m["spot"]))
    # R6: autonomous moves per rolling hour (summons are the session's, not hers).
    auto = [m["t"] for m in moves if not m["goal"].startswith("summon")]
    for i, t in enumerate(auto):
        n = sum(1 for x in auto[: i + 1] if x > t - 3600)
        if n > opt["--max-moves"]:
            bad.append((t, f"R6 {n} autonomous moves in the hour before", ""))
    # R7: bids apart (unless forced).
    bids = [m for m in moves if m["goal"] == "bid"]
    free_bids = [m["t"] for m in bids if not was_forced(m["t"], "Force(Seek)")]
    for a_, b_ in zip(free_bids, free_bids[1:]):
        if b_ - a_ < opt["--bid-gap"] - 1e-6:
            bad.append((b_, f"R7 bid only {b_ - a_:.0f} s after the last", ""))
    # R8: passive moves apart from the move before (unless forced or the far move after a sulk).
    prev = None
    for m in moves:
        if m["goal"] == "passive" and prev is not None and not was_forced(m["t"], "Force(Roam)"):
            if m["t"] - prev < opt["--roam-min"] - 1e-6:
                bad.append((m["t"], f"R8 passive move {m['t'] - prev:.0f} s after the last move", ""))
        prev = m["t"]
    # R9: never parked over his window after a bid: a covering arrival is followed by a move.
    for i, r in enumerate(arrivals):
        if r.get("covering"):
            nxt = [m for m in moves if m["t"] > r["t"]]
            ends = [x for x in rows if x.get("k") == "brain" and x["t"] > r["t"]
                    and any(w in x["what"] for w in ("emote Satisfied", "emote Sulk", "summon over", "linger over", "away"))]
            if ends:
                end = ends[0]["t"]
                left = [m for m in nxt if m["t"] >= end - 1e-6 and m["t"] <= end + 30]
                if not left:
                    bad.append((end, "R9 stayed over his window after the bid ended", r["spot"]))
    # R10: (approach, arrival, emote) never the same as one of the last 2.
    combos = []
    for r in arrivals:
        if r.get("approach") is None:
            continue
        e = next((x["key"] for x in emotes if x["t"] >= r["t"] - 1e-6), None)
        combos.append((r["t"], r["approach"], r["arrival"], e))
    for i, c in enumerate(combos):
        for d in combos[max(0, i - 2): i]:
            if c[1:] == d[1:]:
                bad.append((c[0], f"R10 combo {c[1:]} repeats one of the last 2", ""))
    # R11: fullscreen hide.
    hides = [r for r in rows if r.get("k") in ("hide", "show")]
    print(f"moves {len(moves)} (passive {sum(m['goal'].startswith('passive') for m in moves)}, bids {len(bids)}, "
          f"summons {sum(m['goal'].startswith('summon') for m in moves)}, leaves {sum(m['goal'] == 'leave' for m in moves)}); "
          f"arrivals {len(arrivals)}; emotes {len(emotes)}; hide/show {len(hides)}; forced {len(forced)}")
    if combos:
        print("bid/summon combos (approach, arrival, first emote):")
        for c in combos:
            print(f"  t={c[0]:9.1f}  {c[1]:<17} {c[2]:<9} {c[3]}")
        print(f"  distinct: {len(set(c[1:] for c in combos))} of {len(combos)}")
    print(f"hard-rule violations: {len(bad)}")
    for b in bad:
        print(f"  t={b[0]:9.1f}  {b[1]} {b[2]}")
    return 1 if bad else 0


sys.exit(main())
