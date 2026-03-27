#!/bin/bash
# CoreSAN Testbed — build vmm-san + san-testbed, then run tests.
#
# Usage:
#   ./tools/run-san-test.sh                     # Run all 10 scenarios
#   ./tools/run-san-test.sh --scenario <name>   # Run a single scenario
#   ./tools/run-san-test.sh --interactive        # Interactive CLI mode
#   ./tools/run-san-test.sh --nodes 5            # Interactive with 5 nodes
#   ./tools/run-san-test.sh --unit-tests         # Run only unit tests (no testbed)
#   ./tools/run-san-test.sh --all                # Unit tests + all scenarios
#
# No sudo, no VMs, no real disks needed. Uses temp dirs as fake disks.

set -e
cd "$(dirname "$0")/.."
ROOT="$(pwd)"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

info()  { echo -e "${CYAN}[testbed]${NC} $*"; }
ok()    { echo -e "${GREEN}[testbed]${NC} $*"; }
warn()  { echo -e "${YELLOW}[testbed]${NC} $*"; }
fail()  { echo -e "${RED}[testbed]${NC} $*"; }

# ── Parse args ─────────────────────────────────────────────────

MODE="scenarios"      # scenarios | interactive | unit-tests | all
SCENARIO="all"
NODES=3
EXTRA_ARGS=()

while [[ $# -gt 0 ]]; do
    case $1 in
        --interactive|-i)
            MODE="interactive"
            shift ;;
        --unit-tests|-u)
            MODE="unit-tests"
            shift ;;
        --all|-a)
            MODE="all"
            shift ;;
        --scenario|-s)
            MODE="scenarios"
            SCENARIO="$2"
            shift 2 ;;
        --nodes|-n)
            NODES="$2"
            shift 2 ;;
        --help|-h)
            echo "CoreSAN Testbed"
            echo ""
            echo "Usage:"
            echo "  ./tools/run-san-test.sh                     Run all 10 scenarios"
            echo "  ./tools/run-san-test.sh -s <name>           Run a single scenario"
            echo "  ./tools/run-san-test.sh -i                  Interactive CLI mode"
            echo "  ./tools/run-san-test.sh -n 5                Interactive with 5 nodes"
            echo "  ./tools/run-san-test.sh -u                  Unit tests only"
            echo "  ./tools/run-san-test.sh -a                  Unit tests + all scenarios"
            echo ""
            echo "Scenarios: quorum-degraded, quorum-fenced, quorum-recovery,"
            echo "  fenced-write-denied, fenced-read-allowed, leader-failover,"
            echo "  partition-majority, partition-witness-2node, replication-basic,"
            echo "  repair-leader-only"
            exit 0 ;;
        *)
            EXTRA_ARGS+=("$1")
            shift ;;
    esac
done

# ── Step 1: Build vmm-san ─────────────────────────────────────

info "Building vmm-san..."
cargo build -p vmm-san 2>&1 | tail -3
ok "vmm-san built"

# ── Step 2: Run unit tests (if requested) ─────────────────────

run_unit_tests() {
    info "Running vmm-san unit tests..."
    if cargo test -p vmm-san 2>&1 | tee /tmp/san-unit-test.log | tail -5; then
        RESULT=$(grep "test result:" /tmp/san-unit-test.log | tail -1)
        ok "Unit tests: $RESULT"
        return 0
    else
        fail "Unit tests FAILED"
        cat /tmp/san-unit-test.log
        return 1
    fi
}

if [[ "$MODE" == "unit-tests" ]]; then
    run_unit_tests
    exit $?
fi

if [[ "$MODE" == "all" ]]; then
    run_unit_tests || exit 1
    echo ""
fi

# ── Step 3: Build san-testbed ─────────────────────────────────

info "Building san-testbed..."
cargo build -p san-testbed 2>&1 | tail -3
ok "san-testbed built"

TESTBED="$ROOT/target/debug/san-testbed"

if [[ ! -f "$TESTBED" ]]; then
    fail "san-testbed binary not found at $TESTBED"
    exit 1
fi

# ── Step 4: Run testbed ───────────────────────────────────────

if [[ "$MODE" == "interactive" ]]; then
    info "Starting interactive testbed with $NODES nodes..."
    echo ""
    exec "$TESTBED" --nodes "$NODES" "${EXTRA_ARGS[@]}"

elif [[ "$MODE" == "scenarios" || "$MODE" == "all" ]]; then
    info "Running scenario: $SCENARIO"
    echo ""
    "$TESTBED" --scenario "$SCENARIO" "${EXTRA_ARGS[@]}"
    EXIT=$?
    echo ""
    if [[ $EXIT -eq 0 ]]; then
        ok "All scenarios passed!"
    else
        fail "Some scenarios failed (exit code $EXIT)"
    fi
    exit $EXIT
fi
