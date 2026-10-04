#!/usr/bin/env python3
"""Run exhaustive safety checks and require the named negative-control failures."""
import argparse
import hashlib
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

CASES = {
    "Delivery": None,
    "Delivery-unauthorized": "AuthorizedIntent",
    "Delivery-payloadTarget": "ImmutableDestination",
    "Delivery-lateLookup": "ImmutableDestination",
    "Delivery-staleCheck": "AuthorizedAtAdmission",
    "Delivery-reuseVersion": "AuthorizedAtAdmission",
    "Delivery-emailWitness": "NoCrossChannelDelivery",
    "Delivery-twoDestinationsWitness": "NoTwoDestinations",
    "Routing": None,
    "Routing-spoofWitness": "NoIgnoredSpoof",
    "Routing-payloadRoute": "TrustedIngress",
    "Routing-omitNamespace": "TrustedIngress",
    "Routing-omitClient": "TrustedIngress",
    "Routing-payloadReturn": "ReturnToOrigin",
    "Routing-destinationReturn": "ReturnToOrigin",
    "Routing-wrongPeer": "AuthenticatedReply",
    "SessionContext": None,
    "SessionContext-closeOthers": "RequestLocalClosure",
    "SessionContext-otherCancelWitness": "NoOtherCancelResume",
    "SessionContext-compactionWitness": "NoCompactedRecovery",
    "SessionContext-wrongReturn": "OwnedReturn",
    "SessionContext-copyOther": "ContextIsolation",
    "SessionContext-rollback": "PreserveCausalAndNewer",
    "SessionContext-unpin": "RetainedWhilePending",
    "SessionContext-fabricate": "NoFabricatedRecovery",
    "SessionContext-globalCancel": "SessionLocalCancellation",
    "SessionContext-globalRevision": "SessionLocalRevision",
    "SessionContext-orderWitness": "NoOutOfOrderCompletion",
    "SessionContext-failureWitness": "NoRecoveryFailure",
    "Waits": None,
    "Continuations": None,
    "Waits-racy": "NoWaitCycle",
    "Waits-retry": "RetryIdentity",
    "Continuations-earlyDispatch": "RecoveryBeforeDispatch",
    "Continuations-retryIdentity": "StableEffectIdentity",
    "Continuations-splitAccept": "AtomicAcceptance",
    "Continuations-duplicate": "AtMostOneClaim",
    "Continuations-duplicateResume": "AtMostOneResume",
    "Continuations-stale": "ValidAcceptance",
    "Continuations-staleResume": "ValidResume",
    "Continuations-overwrite": "PreserveNewerActivity",
    "Continuations-correlation": "ValidAcceptance",
    "Continuations-missingCall": "ToolCallResultPairing",
    "Continuations-advancedWitness": "NoAdvancedReplyAccepted",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("jar", type=Path, help="Official tla2tools.jar")
    parser.add_argument("--case", choices=CASES)
    args = parser.parse_args()
    jar = args.jar.resolve(strict=True)
    root = Path(__file__).resolve().parent
    output = root / "results"
    output.mkdir(exist_ok=True)
    print("Tool SHA-256:", hashlib.sha256(jar.read_bytes()).hexdigest(), flush=True)
    cases = {args.case: CASES[args.case]} if args.case else CASES
    failed = []
    for case, expected in cases.items():
        module = case.split("-")[0]
        with tempfile.TemporaryDirectory(prefix="brook-tlc-") as temp:
            for filename in (module + ".tla", case + ".cfg"):
                shutil.copyfile(root / filename, Path(temp) / filename)
            cmd = ["java", "-XX:+UseParallelGC", "-Xmx1g", "-cp", str(jar),
                   "tlc2.TLC", "-workers", "1", "-seed", "1", "-fp", "0",
                   "-noGenerateSpecTE", "-difftrace", "-config", case + ".cfg",
                   module + ".tla"]
            run = subprocess.run(cmd, cwd=temp, text=True, stdout=subprocess.PIPE,
                                 stderr=subprocess.STDOUT, timeout=180)
        # Preserve diagnostics and traces without publishing local machine paths.
        log = re.sub(r"^Parsing file .*$", "Parsing standard or local module",
                     run.stdout, flags=re.MULTILINE)
        log = re.sub(r" \[pid: \d+\]", "", log)
        log = log.replace(str(jar), "<tla2tools.jar>").replace(temp, "<run-dir>")
        (output / (case + ".log")).write_text(log)
        if expected is None:
            ok = (run.returncode == 0 and
                  "Model checking completed. No error has been found." in log and
                  "0 states left on queue." in log)
        else:
            ok = (run.returncode != 0 and
                  f"Invariant {expected} is violated." in log and
                  "State 1:" in log)
        counts = re.findall(r"^[\d,]+ states generated.*$", log, re.MULTILINE)
        print(f"{'PASS' if ok else 'FAIL'} {case}: " +
              (counts[-1] if counts else "no completed state counts") +
              (f" Expected counterexample: {expected}" if expected else ""), flush=True)
        if not ok:
            failed.append(case)
    if failed:
        raise SystemExit("Unexpected results: " + ", ".join(failed))


if __name__ == "__main__":
    main()
