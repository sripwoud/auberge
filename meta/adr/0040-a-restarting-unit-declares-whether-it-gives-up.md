# ADR-0040: A restarting unit declares whether it gives up

## Status

Accepted, 2026-08-25. **Applies ADR-0028's declared regime to the start rate limiter** — the fourth fact about a unit the repo cannot read off its template, after restart edges (ADR-0028), directory writability (ADR-0035) and clean-shutdown exit status (ADR-0038).

## Decision

`tests/start_limit.rs` computes, for every unit the repo configures — a `.service` it templates or copies under `/etc/systemd/system` or a user's `~/.config/systemd/user`, **plus every `.conf` drop-in it installs over one** — the limiter the repo writes for it: `Restart=` and `RestartSec=` from `[Service]`, `StartLimitIntervalSec=` and `StartLimitBurst=` from `[Unit]`, unit file first and then drop-ins in the order systemd loads them, so the last assignment is the effective one.

Two regimes, each named and justified once, and held by unit in `START_LIMIT_REGIMES` (18 entries):

| regime         | limiter                                            | units          |
| -------------- | -------------------------------------------------- | -------------- |
| Restarting App | `StartLimitIntervalSec=3600`, `StartLimitBurst=30` | 17             |
| Resolver       | `StartLimitIntervalSec=0`                          | `blocky` alone |

The pairing is asserted in both directions:

- a unit the repo templates that restarts must appear in the table;
- a table entry must be a unit the repo configures, and — where the repo writes the unit file — one that restarts;
- the limiter the repo writes must equal the regime's, exactly, `[Unit]`-scoped, with no `StartLimitBurst` on the regime that turns the limiter off;
- `(burst - 1) × RestartSec < StartLimitIntervalSec`, read off the files rather than off the declaration, so the arithmetic still holds when a regime's numbers are edited;
- a unit with no declared regime must carry no limiter from this repo;
- a regime no unit holds any more fails, so the table cannot outlive the last unit that needed it.

**A unit the repo does not template joins through its drop-in.** Such a unit's `Restart=` is not in the repo, so it cannot enter the domain by computation; it is classified by hand instead, into one of two lists — a Start Limit Regime, or `UNRESTARTED_ADOPTED_UNITS` with what backs the claim that nothing restarts it (`icecast2`, whose Debian unit sets `Restart=no`, measured). A drop-in over a packaged unit that is on neither list fails the build, and an entry the repo no longer drops in over fails too. Without that pair of lists the navidrome class is unfenced, which is exactly how the fleet's most unreachable limiter came to be found by reading a unit on the Host rather than by this test.

Its drop-in must also pin `RestartSec`, because the arithmetic is measured against that number and one left to the packaging is one the fence would vouch for without being able to read it. navidrome's is pinned at 10s rather than at upstream's 120s, which is a choice and not a consequence: at 120s a burst of 30 spans 58 minutes, so the unit would be down and unreported for most of an hour — a blind spot only slightly smaller than the one being removed. 10s puts it on the fleet's own cadence and its verdict at 290s.

Six reads are hard stops rather than silent misreads: a unit name that does not resolve (ADR-0038's reason — a `loop:` the scan cannot expand would drop out of the domain unseen), a `Restart=` value systemd does not define (a typo would read as "never restarts"), a time-span suffix the parser does not know (`5min` read as 5 makes an unreachable window look reachable), a bare `Key=` (systemd resets the setting to its default; this scan would read the empty value as a deliberate zero, which is the Resolver regime), a `StartLimitBurst` of 0 or 1 (0 turns the limiter off under another name, 1 gives up before a single retry), and a unit written from a task's inline `content:` rather than from a file — the scan reads `src`, so such a unit would leave the domain unseen.

## Why

systemd's default limiter is `DefaultStartLimitBurst=5` inside `DefaultStartLimitIntervalSec=10s`. Any unit setting `RestartSec=5` makes it **unreachable by arithmetic**: four inter-start gaps of 5s already span 20s, so the 10s window can never hold five starts however fast the App fails. 15 of the fleet's 17 templated restarting units carried exactly that.

The cost is not a long loop, it is a blind spot. A unit in auto-restart is `ActiveState=activating`, never `failed`, so it never reaches `systemctl --failed` — the fleet's health signal, and the one ADR-0038 had just finished repairing from the other side. grimmory ran the shape for real: a jar deployed without `static/images/icons/svg/` fails BookLore's `moveIconsToDataFolder` migration, the migration is not recorded when it fails, and so every boot retried it — **4628 `status=1` exits between 13:41 and 20:44 on 2026-08-22, roughly 80 CPU-hours, the App down the whole time, `systemctl --failed` clean throughout.**

This is #635's signal from the other side: that was a false positive every backup window, this is a false negative for seven hours while an App was hard down.

### navidrome, which no template would have shown

Extending the scan to drop-ins was not tidiness. `navidrome.service` comes from the `.deb`, so the audit behind #642 — which read templates — never saw it, and it is the fleet's **worst** case: upstream sets `StartLimitInterval=5` against `RestartSec=120`, so spending its burst of 10 needs 18 minutes inside a five-second window. Unreachable by a factor of 216, retrying every two minutes without end, and invisible.

That is also why the model reads drop-ins in load order rather than unit files alone: for navidrome the drop-in is the only place the repo can speak.

### The sizing, and why it came out uniform

The ticket expected sizing to vary — "an ingress or resolver wants patience, a background oneshot's consumer wants a fast verdict". The discriminator does not survive the fleet. Only 8 of 17 restarting units declare a local `Requires=` at all (tailscaled, redis + postgresql, mysql, icecast2), and a dependency being `active` is not it being _ready_: postgres recovers, mysql replays InnoDB, tailscaled negotiates. The other 9 order on `network.target`, which guarantees neither routes nor names — and blocky, the thing that answers the names they dial at startup, is itself one of the units in question. A short window converts a slow boot into a permanent `failed`, which is a new outage class rather than a repair.

So 30 starts inside an hour: 145s to 290s of retrying at the fleet's `RestartSec` of 5s or 10s, far more attempts than a deterministic failure needs — attempt two fails identically — and more wall clock than any warmup the fleet has. Verdict latency costs nothing, because nobody watches `systemctl --failed` second by second; #644 reads it when the operator is looking.

`liquidsoap`'s existing `600`/`5` moved with the rest, which is a change to a working precedent and deliberate. **`StartLimitBurst` counts every start, not only the automatic ones**: five `auberge deploy radio` runs inside ten minutes would have met `start request repeated too quickly` and left the App down for a reason unrelated to it being broken. A burst sized for crash loops has to leave room for deploys.

`blocky` keeps its opt-out, now with the reason in the repo rather than inferred from a bare `0`: everything resolves through it, including the deploy that would repair it and the operator's own tooling, so a terminal `failed` takes away the path to recovery along with the App — and it buys least there anyway, because a resolver that cannot start is not silent.

### `[Unit]`, measured rather than assumed

The settings moved from `[Service]` to `[Unit]` in systemd v229, and the compatibility story is not the obvious one. Measured on auberge under systemd 257, one transient unit per case, against a 10s/5 baseline:

| written where                               | `StartLimitIntervalSec`         | `StartLimitBurst` |
| ------------------------------------------- | ------------------------------- | ----------------- |
| `[Unit]`                                    | honoured (777s)                 | honoured (17)     |
| `[Service]`                                 | **ignored** — falls back to 10s | honoured (17)     |
| `[Service]`, as legacy `StartLimitInterval` | honoured (777s)                 | honoured (17)     |

So the pair _splits_. A limiter written in `[Service]` with the modern spelling keeps its custom burst and silently keeps the 10s default window — reconstructing the unreachable limiter this ADR exists to remove, written by somebody who believed they had configured it. Both the section and the legacy spelling are fenced: the legacy name is honoured and this scan reads only the modern one, so a unit written that way would be configured in a manner the fence cannot see, which is worse than one misconfigured in a manner it can.

### Demonstrated, not argued

Two transient units on auberge, identical but for the limiter, both `Restart=on-failure`, `RestartSec=5`, both exec'ing `/bin/false`:

|        | adopted `3600`/`30`                   | control, systemd's `10s`/`5`                          |
| ------ | ------------------------------------- | ----------------------------------------------------- |
| t+150s | `activating`, 28 restarts             | `activating`, 28 restarts                             |
| t+175s | **`failed`, in `systemctl --failed`** | `activating`, 33 restarts                             |
| t+251s | `failed`, frozen at 30                | `activating`, **47 restarts**, absent from `--failed` |

```
looptest-adopted.service: Scheduled restart job, restart counter is at 30.
looptest-adopted.service: Start request repeated too quickly.
Failed to start looptest-adopted.service.
```

The control blew nine times past its burst of five and never appeared in `systemctl --failed` once.

## Considered alternatives

- **Size per unit by what it waits on** — a patient regime for the units with a local `Requires=`, a fast verdict for the rest. Rejected on the fleet: the split does not hold (above), and the units it would have hurried are the ones whose startup DNS depends on another unit in the same list.
- **Declare a wall-clock patience and compute `burst` from `RestartSec`.** Rejected: the burst is what an operator reads in the unit file, so deriving it puts the number they see one indirection away from the number the repo decided.
- **Lower `RestartSec` until systemd's own 10s window becomes reachable.** Rejected: five starts inside ten seconds means giving up in under ten seconds. No warmup survives it, and every recovery becomes a hair trigger.
- **Leave the limiter alone and watch `NRestarts` instead**, or alert on `activating (auto-restart)`. Rejected: it builds a second health signal beside the one systemd already has and leaves `systemctl --failed` still lying. #644 reads the state this ADR makes reachable; it does not replace it.
- **Turn the limiter off fleet-wide**, blocky's answer for everyone. Rejected: that is today's behaviour made explicit. The endless loop is the defect, not the reporting of it.
- **`Restart=no` for units that should not self-heal.** Rejected: they should self-heal. The question was only when to stop.
- **Pin navidrome at upstream's `RestartSec=120`** instead of 10s, since 29 gaps of 120s still fit inside the hour and the fence would pass. Rejected: it would take 58 minutes to reach the verdict, which keeps most of the blind spot this ADR exists to close.
- **A `why` per unit** rather than per regime. Rejected: it would be 17 restatements of one sentence, and the sizing follows from what depends on a unit, not from the App behind it.
- **Read only unit files, as ADR-0038 does.** Rejected once navidrome was found: the fleet's most unreachable limiter is on the one restarting unit the repo drops in over rather than templates, and a fence that cannot see it would have reported the fleet clean.

## Consequences

**Positive:**

- The build now fails on: a new restarting unit with no regime, a drop-in over a packaged unit that is classified neither way, a table entry naming a unit the repo does not configure or does not restart, a limiter that does not match its regime, an unreachable `(burst - 1) × RestartSec`, a limiter outside `[Unit]`, the legacy spelling, a limiter on a unit with no regime, a regime nobody holds, and each of the six hard stops. Ten tests.
- A hard-down App reaches `failed` in 145s to 290s and appears in `systemctl --failed`, demonstrated above. The grimmory shape is capped at 30 attempts instead of 4628.
- navidrome's limiter becomes reachable for the first time, and its `RestartSec` drops from upstream's 120s to the fleet's 10s.
- Iterative deploys of the radio no longer risk `start request repeated too quickly`.

**Negative:**

- **navidrome's limiter is largely masked by a separate defect this ADR does not fix.** Its packaged unit carries `SuccessExitStatus=1 2 8 SIGKILL`, so an ordinary crash — and a `MemoryMax` kill under ADR-0021 — scores as success, `Restart=on-failure` never fires, and the unit stops clean instead of looping or failing. The limiter governs only the statuses outside that list. Clearing the whitelist is ADR-0038's model applied to a unit outside its scan, and is tracked separately rather than folded in here.
- 30 is a judgement, not a measurement. No unit in this fleet has a measured warmup ceiling; the number is sized to clear the warmups that exist by a wide margin, not to fit one.
- `blocky` can still loop without end, by design, and stays absent from `systemctl --failed` while it does. That is the regime; the cost is recorded rather than removed.
- The table binds to role and unit names, so adding, removing or renaming a unit fails this test until the list moves with it. Loud and cheap, the trade ADR-0028 already accepted.
- A unit the fleet runs but neither templates **nor drops in over** is still outside the model, silently — apt's `php-fpm` behind baikal and yourls, and the substrate units (`mysql`, `redis-server`, `tailscaled`, `postgresql`, `icecast2`). Of those, only `mysql` carries an unreachable limiter, and its `Restart=on-abnormal` does not fire on the nonzero exit a broken config produces. Recorded rather than closed: the repo owns no file for them.
- A `Restart=` written anywhere but `[Service]` is read as absent. systemd would ignore it too, so the unit genuinely would not restart — but the author's intent would be lost with nothing to catch it.

## References

- Issue #642 — the unreachable default, grimmory's seven-hour window, and the fleet audit that put 15 of 17 units in it.
- Issue #644 — reading a unit's state out at deploy time, which this ADR makes worth reading.
- ADR-0028 — the declared regime: computed domain, per-entry justification, equality in both directions.
- ADR-0038 — the same regime over `SuccessExitStatus`, and the health signal this repairs from the other side.
- ADR-0035 — the precedent for pinning a computed domain by set equality rather than by count.
- ADR-0021 — Memory Budgets; a `MemoryMax` kill is a signal death, which is why navidrome's `SIGKILL` whitelist matters above.
