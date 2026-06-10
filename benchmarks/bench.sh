#!/usr/bin/env bash
set -euo pipefail

# Benchmark script comparing aube, pnpm, yarn (berry), npm, bun, deno,
# and vlt install performance.
#
# Prerequisites:
#   - aube built in release mode: cargo build --release
#   - benchmark dependencies from mise (use `mise run bench` or
#     `mise run bench:bump`; missing package managers are skipped with
#     a warning rather than failing the whole run)
#
# Usage:
#   mise run bench
#
# Environment variables:
#   WARMUP       — warmup runs before timing (default: 1)
#   RUNS         — timed runs per benchmark (default: 10), applied to
#                  EVERY tool equally. The old per-tool taper (pnpm at
#                  half, npm/yarn at a third) saved wall time but made
#                  the slow tools' statistics structurally noisier than
#                  the fast tools' — a published ratio should never
#                  compare a 10-run median to a 4-run one. If wall time
#                  hurts, cut scenarios, not statistical symmetry.
#   RUNS_PNPM, RUNS_NPM, RUNS_YARN, RUNS_BUN, RUNS_AUBE, RUNS_NUB,
#   RUNS_DENO, RUNS_VLT — override the per-tool run count individually
#                  (escape hatch; published runs keep them equal).
#   RESULTS_JSON — override the structured JSON output path
#   BENCH_TOOLS  — comma-separated tools to include
#                  (default: aube,bun,pnpm,npm,yarn,deno; vlt is
#                  temporarily disabled — its --frozen-lockfile still
#                  makes network requests, skewing results; nub is
#                  opt-in — add it explicitly, e.g.
#                  BENCH_TOOLS=nub,aube,pnpm)
#   BENCH_NUB_BIN — path to the nub binary (the Rust CLI that embeds
#                  the aube install engine; `nub install` routes
#                  through `aube::commands::install` in-process).
#                  The benchmark runner is expected to pass this
#                  explicitly. Default: ../nub/target/release/nub
#                  relative to this repo's parent (the side-by-side
#                  scratch-clone layout), falling back to `nub` on
#                  $PATH. nub's native lockfile is pnpm-lock.yaml
#                  (its embedder default is defaultLockfileFormat=pnpm).
#   BENCH_NUB_ENGINE_VERSION — optional embedded-aube version string
#                  recorded into results.json `versions` as
#                  "nub-aube-engine" (the binary's --version prints
#                  only the nub version; the runner knows the vendored
#                  submodule rev and passes it through).
#   BENCH_SCENARIOS — comma-separated scenario keys to run
#                     (default: gvs-warm,gvs-cold,install-test).
#                     Opt-in keys beyond the default set:
#                       ci-loop       — S10: warm CI loop (install
#                                       short-circuit + test dispatch),
#                                       emitted as TWO rows: ci-loop-noci
#                                       (CI unset) and ci-loop-ci
#                                       (CI=true) — the env flip is the
#                                       measurement. GVS is deliberately
#                                       unpinned here so each tool's own
#                                       CI heuristic shows up.
#                       add-dep       — S11: warm everything, inject one
#                                       pinned dep (left-pad@1.3.0) into
#                                       package.json, time the non-frozen
#                                       install. package.json edit, not
#                                       each tool's `add` verb, so every
#                                       tool does identical work.
#                       branch-switch — S12: node_modules settled for
#                                       lockfile A, swap in the committed
#                                       fixture-b.package.json (~15
#                                       version deltas) + its pre-built
#                                       native lockfile B, time the
#                                       incremental install.
#   BENCH_PHASES — set to 0 to skip aube phase timing samples
#   BENCH_FIXTURE — which fixture to bench. Default: the single-package
#                  fixture.package.json (with fixture-b.package.json as
#                  its branch-B variant). Any other value names a
#                  directory under benchmarks/fixtures/ that is copied
#                  wholesale into each tool's project dir, with an
#                  optional sibling `<name>-b` directory as the
#                  branch-B variant. `workspace-descript` is the
#                  5-member pnpm workspace (Electron-app shell, two
#                  peer-heavy React UI members, an express/prisma
#                  service, a tooling member; ~230 direct deps, ~2.9k
#                  resolved; committed REAL-pnpm-generated
#                  pnpm-lock.yaml in both A and B variants — the
#                  committed lockfile is seeded as pnpm's and nub's
#                  benchmarked lockfile, which makes it the
#                  foreign-lockfile testbed for nub). Workspace
#                  fixtures use workspace:* ranges + pnpm-workspace.yaml;
#                  run them with BENCH_TOOLS drawn from
#                  nub,aube,pnpm,bun,yarn — npm/deno/vlt don't speak
#                  that combination and will fail populate.
#
#   BENCH_HERMETIC=1 — route all registry traffic through a local
#                      Verdaccio instance pre-populated from npmjs. This
#                      is the default for mise tasks; leave it on so
#                      cold-cache numbers are not npmjs/CDN latency tests.
#                      First hermetic run warms the cache at
#                      ~/.cache/aube-bench/registry/; subsequent runs
#                      are fully offline. See benchmarks/hermetic.bash.
#   BENCH_BANDWIDTH  — optional throttle (e.g. `50mbit`, `6mbit`, bare
#                      integer bytes/s). Defaults to `500mbit` in mise
#                      tasks; routes traffic through a tiny token-bucket
#                      proxy in front of Verdaccio.
#   BENCH_LATENCY    — optional fixed response latency for the throttle
#                      proxy. Defaults to `50ms` in mise tasks.
#
# Config-tier plumbing (every published number carries its tier label;
# the runner invokes bench.sh once per tier/cell with RESULTS_JSON set
# to a distinct path — the knobs below are recorded into results.json's
# `environment` block):
#
#   BENCH_TIER       — convenience defaults-setter. `t1` = out-of-box
#                      (GVS auto, advisory check at tool defaults,
#                      minimum-release-age unpinned: every tool ships
#                      its own default). `t2` = normalized same-work
#                      (GVS pinned on for every tool that has one,
#                      advisory check off, minimum-release-age pinned
#                      equal). Explicit BENCH_GVS / BENCH_ADVISORY_CHECK /
#                      BENCH_MIN_RELEASE_AGE_MINUTES always win over the
#                      tier default. Unset = legacy behavior (pin-fast).
#   BENCH_GVS        — global-virtual-store axis, the four-cell knob:
#                      `pin-fast` (default; aube/nub pinned on, pnpm at
#                      its default off — upstream's published config),
#                      `on` (aube/nub/pnpm all pinned on), `off` (all
#                      pinned off), `auto` (nothing pinned; each tool's
#                      own heuristic decides — note CI is scrubbed from
#                      the env, see below). pnpm's equivalent setting is
#                      enableGlobalVirtualStore (experimental ≥10.12).
#                      The ci-loop scenario ignores this knob by design.
#   BENCH_ADVISORY_CHECK — `default` (aube/nub keep their on-by-default
#                      OSV MAL-* check; it fires only on fresh-resolution
#                      flows) or `off` (pinned off for aube/nub — the T2
#                      same-work setting; pnpm/npm/bun have no equivalent
#                      to pin, npm's audit is already off via --no-audit).
#   BENCH_MIN_RELEASE_AGE_MINUTES — numeric pin applied to every PM that
#                      supports the gate (existing behavior, default
#                      1440), `0` to disable everywhere, or `default` to
#                      pin nothing anywhere: aube/nub keep their
#                      compiled-in 1440, everyone else keeps their own
#                      default (the T1 out-of-box posture — aube alone
#                      pays its full-packument cost, priced honestly).
#
# CI env policy: `CI` is unset for the entire run — it is a benchmark
# *variable*, not ambient state (GitHub Actions' inherited CI=true used
# to silently flip tool heuristics). The only place it appears is the
# ci-loop scenario's explicit CI=true row.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
AUBE_BIN="$REPO_DIR/target/release/aube"
# nub lives in its own repo; the runner passes BENCH_NUB_BIN. The default
# assumes the side-by-side scratch layout (<workdir>/aube + <workdir>/nub).
NUB_BIN="${BENCH_NUB_BIN:-$REPO_DIR/../nub/target/release/nub}"
if [ ! -x "$NUB_BIN" ]; then
	NUB_BIN="$(command -v nub || true)"
fi
PNPM_BIN="$(command -v pnpm || true)"
YARN_BIN="$(command -v yarn || true)"
NPM_BIN="$(command -v npm || true)"
BUN_BIN="$(command -v bun || true)"
DENO_BIN="$(command -v deno || true)"
VLT_BIN="$(command -v vlt || true)"

BENCH_DIR="$(mktemp -d "${TMPDIR:-/tmp}/aube-bench.XXXXXX")"
WARMUP="${WARMUP:-1}"
RUNS="${RUNS:-10}"
# Every tool gets the same run count — equal-N or the comparison is
# apples-to-oranges statistically (see the RUNS doc comment above).
# Per-tool overrides remain as an escape hatch for local iteration.
RUNS_AUBE="${RUNS_AUBE:-$RUNS}"
RUNS_NUB="${RUNS_NUB:-$RUNS}"
RUNS_BUN="${RUNS_BUN:-$RUNS}"
RUNS_DENO="${RUNS_DENO:-$RUNS}"
RUNS_PNPM="${RUNS_PNPM:-$RUNS}"
RUNS_VLT="${RUNS_VLT:-$RUNS}"
RUNS_NPM="${RUNS_NPM:-$RUNS}"
RUNS_YARN="${RUNS_YARN:-$RUNS}"
BENCH_TOOLS="${BENCH_TOOLS:-aube,bun,pnpm,npm,yarn,deno}"
BENCH_SCENARIOS="${BENCH_SCENARIOS:-gvs-warm,gvs-cold,install-test}"
BENCH_PHASES="${BENCH_PHASES:-1}"

# ── Config tiers ────────────────────────────────────────────────────────────
# BENCH_TIER fills in defaults for the three normalization knobs; an
# explicitly set knob always wins. See the header comment for semantics.

BENCH_TIER="${BENCH_TIER:-}"
case "$BENCH_TIER" in
"") ;;
t1)
	BENCH_GVS="${BENCH_GVS:-auto}"
	BENCH_ADVISORY_CHECK="${BENCH_ADVISORY_CHECK:-default}"
	BENCH_MIN_RELEASE_AGE_MINUTES="${BENCH_MIN_RELEASE_AGE_MINUTES:-default}"
	;;
t2)
	BENCH_GVS="${BENCH_GVS:-on}"
	BENCH_ADVISORY_CHECK="${BENCH_ADVISORY_CHECK:-off}"
	BENCH_MIN_RELEASE_AGE_MINUTES="${BENCH_MIN_RELEASE_AGE_MINUTES:-1440}"
	;;
*)
	echo "error: BENCH_TIER must be t1, t2, or unset (got: $BENCH_TIER)" >&2
	exit 1
	;;
esac
BENCH_GVS="${BENCH_GVS:-pin-fast}"
BENCH_ADVISORY_CHECK="${BENCH_ADVISORY_CHECK:-default}"
export BENCH_TIER BENCH_GVS BENCH_ADVISORY_CHECK

# CI is a benchmark variable, not ambient state. Tools flip real
# behavior on it (aube's GVS heuristic, yarn's immutable installs,
# bun's frozen lockfile), so an inherited CI=true from GitHub Actions
# would silently change what's being measured. The ci-loop scenario
# re-introduces it explicitly as its own labeled row.
unset CI

# The workspace-descript fixture carries electron in devDependencies.
# Lifecycle scripts are off in every scenario, so its postinstall (the
# binary CDN download) never runs — this is belt and suspenders so no
# tool's auto-install path can ever turn a bench run into a CDN test.
export ELECTRON_SKIP_BINARY_DOWNLOAD=1

# ── Fixture selection ───────────────────────────────────────────────────────
# FIXTURE_SRC is either a single package.json (the default fixture) or
# a directory copied wholesale into each tool's project. FIXTURE_B_SRC
# is the branch-B variant used by the branch-switch scenario.
BENCH_FIXTURE="${BENCH_FIXTURE:-default}"
case "$BENCH_FIXTURE" in
default)
	FIXTURE_SRC="$SCRIPT_DIR/fixture.package.json"
	FIXTURE_B_SRC="$SCRIPT_DIR/fixture-b.package.json"
	;;
*)
	FIXTURE_SRC="$SCRIPT_DIR/fixtures/$BENCH_FIXTURE"
	FIXTURE_B_SRC="$SCRIPT_DIR/fixtures/${BENCH_FIXTURE}-b"
	if [ ! -d "$FIXTURE_SRC" ]; then
		echo "error: unknown BENCH_FIXTURE '$BENCH_FIXTURE' (no $FIXTURE_SRC)" >&2
		exit 1
	fi
	;;
esac
if [ ! -e "$FIXTURE_B_SRC" ]; then
	case ",$BENCH_SCENARIOS," in
	*,branch-switch,*)
		echo "error: branch-switch needs a B variant at $FIXTURE_B_SRC" >&2
		exit 1
		;;
	esac
fi
# hermetic.bash reads these: the warm pass installs the active fixture
# (not unconditionally fixture.package.json), and the warm sentinel is
# tagged per fixture so switching fixtures re-warms the registry.
export BENCH_FIXTURE
export BENCH_FIXTURE_SRC="$FIXTURE_SRC"

# ── Validation ──────────────────────────────────────────────────────────────

if ! command -v hyperfine &>/dev/null; then
	echo "error: hyperfine is required. Run via: mise run bench" >&2
	exit 1
fi

# The aube binary is only a hard requirement when aube is actually in
# the tool set — a nub-only run (BENCH_TOOLS=nub,pnpm,...) must not
# demand a local aube build.
case ",$BENCH_TOOLS," in
*,aube,*)
	if [ ! -f "$AUBE_BIN" ]; then
		echo "error: aube release binary not found at $AUBE_BIN" >&2
		echo "Run: cargo build --release" >&2
		exit 1
	fi
	;;
esac

# ── Optional hermetic registry ─────────────────────────────────────────────
# BENCH_HERMETIC=1 routes all registry traffic through a local
# Verdaccio instance (populated from npmjs on first run, offline after).
# BENCH_BANDWIDTH=<rate> and BENCH_LATENCY=<delay> put a throttling
# proxy in front so cold-cache numbers reflect a simulated internet
# link rather than loopback disk speed. See benchmarks/hermetic.bash
# for the lifecycle details.

BENCH_REGISTRY_URL=""
if [ "${BENCH_HERMETIC:-0}" = "1" ]; then
	# shellcheck source=/dev/null
	source "$SCRIPT_DIR/hermetic.bash"
	hermetic_start
	trap 'hermetic_stop' EXIT
fi

# ── Per-tool configuration ─────────────────────────────────────────────────
# Build up the list of tools to include dynamically so the matrix
# gracefully skips any pm that isn't installed. Each tool gets its
# own project dir, HOME, store, and cache so the scenarios are
# hermetic per-tool.

TOOLS=()
TOOL_BINS=()
TOOL_PROJECTS=()
TOOL_HOMES=()
TOOL_STORES=()
TOOL_CACHES=()

register_tool() {
	local name=$1 bin=$2
	case ",$BENCH_TOOLS," in
	*,"$name",*) ;;
	*) return ;;
	esac
	if [ -z "$bin" ] || [ ! -x "$bin" ]; then
		echo "warning: $name not found on \$PATH — skipping" >&2
		return
	fi
	TOOLS+=("$name")
	TOOL_BINS+=("$bin")
	TOOL_PROJECTS+=("$BENCH_DIR/project-$name")
	TOOL_HOMES+=("$BENCH_DIR/home-$name")
	TOOL_STORES+=("$BENCH_DIR/store-$name")
	TOOL_CACHES+=("$BENCH_DIR/cache-$name")
}

run_scenario() {
	local name=$1
	case ",$BENCH_SCENARIOS," in
	*,"$name",*) ;;
	*) return ;;
	esac

	shift
	"$@"
}

# Order matters for the console output; keep aube first so the
# headline comparison is prominent and the rest follow alphabetically.
# nub (when opted in via BENCH_TOOLS) slots in right after aube — the
# nub-vs-aube delta is the fork-overhead regression sentinel.
register_tool "aube" "$AUBE_BIN"
register_tool "nub" "$NUB_BIN"
register_tool "bun" "$BUN_BIN"
register_tool "deno" "$DENO_BIN"
register_tool "pnpm" "$PNPM_BIN"
register_tool "npm" "$NPM_BIN"
register_tool "yarn" "$YARN_BIN"
register_tool "vlt" "$VLT_BIN"

echo "workdir: $BENCH_DIR"
# Capture each tool's reported --version string so generate-results.js
# can fold it into results.json. Some tools print extra text around
# the semver (e.g. `aube 1.0.0-beta.3 (...)`, `bun 1.3.12+...`); the
# sed pulls out the first token that looks like a semver so the JSON
# stays clean without the consumers having to re-parse it.
versions_file="$BENCH_DIR/versions.tsv"
: >"$versions_file"
for i in "${!TOOLS[@]}"; do
	tool="${TOOLS[$i]}"
	bin="${TOOL_BINS[$i]}"
	raw="$($bin --version 2>/dev/null || echo 'unknown')"
	version="$(printf '%s\n' "$raw" | head -n1 | grep -Eo '[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.+-]+)?' | head -n1)"
	[ -z "$version" ] && version="$raw"
	printf "%s\t%s\n" "$tool" "$version" >>"$versions_file"
	printf "%-5s %s  (%s)\n" "$tool:" "$bin" "$version"
done
node_version="$(node --version 2>/dev/null | sed 's/^v//')"
if [ -n "$node_version" ]; then
	printf "%s\t%s\n" "node" "$node_version" >>"$versions_file"
	printf "%-5s %s\n" "node:" "$node_version"
fi
export BENCH_VERSIONS_FILE="$versions_file"
echo ""

runs_for_tool() {
	case "$1" in
	aube) echo "$RUNS_AUBE" ;;
	nub) echo "$RUNS_NUB" ;;
	bun) echo "$RUNS_BUN" ;;
	deno) echo "$RUNS_DENO" ;;
	pnpm) echo "$RUNS_PNPM" ;;
	npm) echo "$RUNS_NPM" ;;
	yarn) echo "$RUNS_YARN" ;;
	vlt) echo "$RUNS_VLT" ;;
	*) echo "$RUNS" ;;
	esac
}

# Per-tool lockfile filename (the name the pm writes into the project
# directory after `install`). Used to decide what to save after the
# populate step and where to copy it back for the "warm lockfile"
# scenarios.
lockfile_name_for() {
	case "$1" in
	aube) echo "aube-lock.yaml" ;;
	# nub's embedder defaults write pnpm-lock.yaml for fresh projects
	# (defaultLockfileFormat=pnpm) — pnpm's format IS nub's native format.
	nub) echo "pnpm-lock.yaml" ;;
	bun) echo "bun.lock" ;;
	deno) echo "deno.lock" ;;
	npm) echo "package-lock.json" ;;
	pnpm) echo "pnpm-lock.yaml" ;;
	yarn) echo "yarn.lock" ;;
	vlt) echo "vlt-lock.json" ;;
	*) echo "unknown" ;;
	esac
}

# ── Project setup ──────────────────────────────────────────────────────────

for i in "${!TOOLS[@]}"; do
	tool="${TOOLS[$i]}"
	dir="${TOOL_PROJECTS[$i]}"
	home="${TOOL_HOMES[$i]}"
	mkdir -p "$dir" "$home" "${TOOL_CACHES[$i]}"
	if [ -d "$FIXTURE_SRC" ]; then
		# Directory fixture (workspace): copy the whole tree — member
		# package.jsons, pnpm-workspace.yaml, and any committed
		# lockfile ride along.
		cp -R "$FIXTURE_SRC/." "$dir/"
	else
		cp "$FIXTURE_SRC" "$dir/package.json"
	fi

	# pnpm reads storeDir / cacheDir from pnpm-workspace.yaml; the
	# other tools take them via CLI flags or env vars at command
	# time, so nothing to write on disk up front. Append rather than
	# overwrite: a workspace fixture ships its own pnpm-workspace.yaml
	# (the `packages:` list) that must survive.
	if [ "$tool" = "pnpm" ]; then
		printf "storeDir: %s\ncacheDir: %s\n" "${TOOL_STORES[$i]}" "${TOOL_CACHES[$i]}" >>"$dir/pnpm-workspace.yaml"
	fi

	# Yarn 4 ignores .npmrc for registry and only ships a PnP linker by
	# default. Drop a .yarnrc.yml that pins node-modules layout (so the
	# scenarios mirror what npm/pnpm/bun produce), routes the cache to
	# the isolated dir, disables telemetry, and turns off lifecycle
	# scripts so it matches the --ignore-scripts behavior we ask from
	# the other tools. The hermetic registry URL gets injected lower
	# down once we know BENCH_REGISTRY_URL.
	if [ "$tool" = "yarn" ]; then
		{
			printf "nodeLinker: node-modules\n"
			printf "cacheFolder: %s\n" "${TOOL_CACHES[$i]}"
			printf "enableGlobalCache: false\n"
			printf "enableTelemetry: false\n"
			printf "enableScripts: false\n"
			# Yarn 4 auto-enables immutable installs when it detects
			# `CI=true`, which makes the warm step refuse to create
			# the initial lockfile (YN0028) and crashes bench-refresh.
			# Pin the flag off so the warm install can populate the
			# lockfile + cache + store the same way it does locally.
			printf "enableImmutableInstalls: false\n"
		} >"$dir/.yarnrc.yml"
	fi

	# Hermetic mode: drop a .npmrc into both the project dir and the
	# isolated HOME so every PM resolves packages through the local
	# Verdaccio (or the throttle proxy in front of it) instead of
	# npmjs. Project-level .npmrc is honored by aube/pnpm/npm/bun/
	# deno/vlt and wins over HOME; HOME is a belt-and-suspenders
	# fallback for any command (like `aube add` after chdir) that
	# might look there first. Yarn 4 ignores .npmrc and reads the
	# registry from .yarnrc.yml instead, so we append it there.
	if [ -n "$BENCH_REGISTRY_URL" ]; then
		printf "registry=%s\n" "$BENCH_REGISTRY_URL" >"$dir/.npmrc"
		printf "registry=%s\n" "$BENCH_REGISTRY_URL" >"$home/.npmrc"
		if [ "$tool" = "yarn" ]; then
			printf "npmRegistryServer: \"%s\"\nunsafeHttpWhitelist:\n  - 127.0.0.1\n  - localhost\n" \
				"$BENCH_REGISTRY_URL" >>"$dir/.yarnrc.yml"
		fi
	fi
done

# Pristine manifest overlays. The staged scenarios (ci-loop, add-dep,
# branch-switch) mutate package.json mid-prepare, so they need a way to
# restore "manifest state A" (and apply "manifest state B") that works
# for both fixture shapes: an overlay dir holding only the fixture's
# package.json files (root + workspace members), applied with
# `cp -R <overlay>/. <project>/`. For the branch-B variant the deltas
# are ~15 direct deps pinned to older exact versions (all comfortably
# past any minimum-release-age gate); each tool's native lockfile B is
# generated during populate (uplink-bracketed) and saved alongside the
# A lockfile.
FIXTURE_A_OVERLAY="$BENCH_DIR/fixture-a-overlay"
FIXTURE_B_OVERLAY="$BENCH_DIR/fixture-b-overlay"
build_fixture_overlay() {
	local src=$1 dst=$2
	mkdir -p "$dst"
	if [ -f "$src" ]; then
		cp "$src" "$dst/package.json"
	else
		(cd "$src" && find . -name package.json -not -path '*/node_modules/*' | tar cf - -T -) | tar xf - -C "$dst"
	fi
}
build_fixture_overlay "$FIXTURE_SRC" "$FIXTURE_A_OVERLAY"
if [ -e "$FIXTURE_B_SRC" ]; then
	build_fixture_overlay "$FIXTURE_B_SRC" "$FIXTURE_B_OVERLAY"
fi

# The single pinned dep the add-dep scenario injects. Ancient, zero
# transitive deps, unscoped (the prefetch below assumes an unscoped
# tarball path) — the measurement is the resolve/packument/link path,
# not this package's size.
ADD_DEP_NAME="left-pad"
ADD_DEP_VERSION="1.3.0"

# ── Populate stores and caches ─────────────────────────────────────────────
# One warm install per tool so the lockfile + cache + store are all
# populated before the scenario matrix runs. Everything is hermetic
# (isolated HOME / cache / store), so this is safe to run in parallel
# in the future but we keep it serial for clear console output.
#
# Bracket the populate loop with uplink-enabled Verdaccio so any
# package the warm step missed (e.g. when a PM's resolution diverges
# between the warm fixture and the bench fixture) gets fetched from
# npmjs and cached locally. The cold config is restored afterwards so
# the timed scenarios remain hermetic.
if [ "${BENCH_HERMETIC:-0}" = "1" ]; then
	hermetic_use_warm_uplink
fi

# One warm (non-frozen) install for $tool in $dir. Factored out of the
# populate loop so the branch-switch B-lockfile generation below can
# reuse the exact same invocations. Runs in a subshell so the `cd`
# doesn't leak.
populate_install() {
	local tool=$1 dir=$2 bin=$3 home=$4 store=$5 cache=$6
	case "$tool" in
	aube)
		(cd "$dir" && HOME="$home" XDG_CACHE_HOME="$cache" XDG_DATA_HOME="$home/.local/share" "$bin" install)
		;;
	nub)
		# Same isolation shape as aube: nub's engine cache lands at
		# $XDG_CACHE_HOME/nub/pm and its CAS store at
		# $XDG_DATA_HOME/nub/store. Scripts are off by default (the
		# embedded engine's default), matching aube.
		(cd "$dir" && HOME="$home" XDG_CACHE_HOME="$cache" XDG_DATA_HOME="$home/.local/share" "$bin" install)
		;;
	npm)
		# `--legacy-peer-deps` is the only way npm tolerates the
		# fixture's mixed peer-dep ranges (eslint 9 vs 8, etc.).
		# pnpm/aube handle this via `autoInstallPeers=true` by
		# default; using npm's strict mode here would just make
		# the populate step fail before we even reach the
		# scenarios. Yes, this is the classic "npm is stricter"
		# caveat you read in every benchmark footnote.
		(cd "$dir" && HOME="$home" npm_config_cache="$cache" "$bin" install \
			--ignore-scripts --no-audit --no-fund --legacy-peer-deps)
		;;
	pnpm)
		(cd "$dir" && HOME="$home" "$bin" install --ignore-scripts --no-frozen-lockfile)
		;;
	yarn)
		# Yarn 4 (berry). enableScripts/cacheFolder/nodeLinker are
		# already pinned in .yarnrc.yml, so we only need to ask for
		# a fresh install here.
		(cd "$dir" && HOME="$home" "$bin" install)
		;;
	bun)
		# Bun takes `--cache-dir` as a CLI flag and `BUN_INSTALL` as
		# the global install prefix. Point both at the hermetic temp
		# to keep it from touching `~/.bun`.
		(cd "$dir" && HOME="$home" BUN_INSTALL="$home/.bun" "$bin" install \
			--cache-dir "$cache" --ignore-scripts --no-summary --force)
		;;
	deno)
		# Deno 2 reads package.json and writes deno.lock + populates
		# node_modules. DENO_DIR is the per-tool cache and global
		# install location. Lifecycle scripts are skipped by default
		# (Deno requires explicit --allow-scripts to opt in).
		(cd "$dir" && HOME="$home" DENO_DIR="$cache" "$bin" install --quiet)
		;;
	vlt)
		# vlt respects npm_config_cache for its package cache and
		# reads .npmrc for the registry. Skips lifecycle scripts by
		# default unless an allowlist is configured.
		(cd "$dir" && HOME="$home" npm_config_cache="$cache" "$bin" install)
		;;
	esac
}

for i in "${!TOOLS[@]}"; do
	tool="${TOOLS[$i]}"
	dir="${TOOL_PROJECTS[$i]}"
	bin="${TOOL_BINS[$i]}"
	home="${TOOL_HOMES[$i]}"
	store="${TOOL_STORES[$i]}"
	cache="${TOOL_CACHES[$i]}"
	lockfile_name=$(lockfile_name_for "$tool")
	echo "Populating store and cache for $tool..."
	# Wipe every known lockfile so an earlier failed run doesn't
	# leave a stale one behind that would fool the pm into a
	# different code path.
	rm -rf "$dir/node_modules" \
		"$dir"/packages/*/node_modules \
		"$dir/pnpm-lock.yaml" \
		"$dir/aube-lock.yaml" \
		"$dir/package-lock.json" \
		"$dir/yarn.lock" \
		"$dir/bun.lock" \
		"$dir/bun.lockb" \
		"$dir/deno.lock" \
		"$dir/vlt-lock.json"

	# Workspace fixtures ship a committed, REAL-pnpm-generated lockfile.
	# Seed it back after the wipe for the tools whose native format it
	# is (pnpm itself, and nub whose native lockfile is pnpm-lock.yaml):
	# the benchmarked lockfile is then the committed one — for nub this
	# is the foreign-lockfile posture, operating against a lockfile that
	# real pnpm wrote — rather than whatever this tool's resolver would
	# freshly pick.
	if [ -d "$FIXTURE_SRC" ] && [ -f "$FIXTURE_SRC/$lockfile_name" ]; then
		cp "$FIXTURE_SRC/$lockfile_name" "$dir/$lockfile_name"
	fi

	populate_install "$tool" "$dir" "$bin" "$home" "$store" "$cache"

	if [ ! -f "$dir/$lockfile_name" ]; then
		echo "error: $lockfile_name was not created for $tool in $dir" >&2
		exit 1
	fi
	cp "$dir/$lockfile_name" "$BENCH_DIR/saved-lockfile-$tool"

	# Branch-switch needs each tool's *native* lockfile for the B
	# fixture too. Generate it while the uplink bracket is still open:
	# start from the settled A state (lockfile + node_modules present)
	# so the B lockfile is the minimal-diff shape a real branch switch
	# would produce, then restore the A package.json + lockfile.
	case ",$BENCH_SCENARIOS," in
	*,branch-switch,*)
		echo "Populating branch-B lockfile for $tool..."
		cp -R "$FIXTURE_B_OVERLAY/." "$dir/"
		# Same committed-lockfile seeding as the A populate above.
		if [ -d "$FIXTURE_B_SRC" ] && [ -f "$FIXTURE_B_SRC/$lockfile_name" ]; then
			cp "$FIXTURE_B_SRC/$lockfile_name" "$dir/$lockfile_name"
		fi
		populate_install "$tool" "$dir" "$bin" "$home" "$store" "$cache"
		if [ ! -f "$dir/$lockfile_name" ]; then
			echo "error: branch-B $lockfile_name was not created for $tool in $dir" >&2
			exit 1
		fi
		cp "$dir/$lockfile_name" "$BENCH_DIR/saved-lockfile-b-$tool"
		cp -R "$FIXTURE_A_OVERLAY/." "$dir/"
		cp "$BENCH_DIR/saved-lockfile-$tool" "$dir/$lockfile_name"
		;;
	esac
done

# The add-dep scenario resolves one extra package at bench time; pull
# its packument + tarball through Verdaccio while the uplink is open so
# the no-uplink timed runs don't 404. (Unscoped tarball path — keep
# ADD_DEP_NAME unscoped or extend this.)
case ",$BENCH_SCENARIOS," in
*,add-dep,*)
	if [ -n "$BENCH_REGISTRY_URL" ]; then
		echo "Prefetching ${ADD_DEP_NAME}@${ADD_DEP_VERSION} into the hermetic registry..."
		curl -fsS "$BENCH_REGISTRY_URL/$ADD_DEP_NAME" -o /dev/null
		curl -fsS "$BENCH_REGISTRY_URL/$ADD_DEP_NAME/-/$ADD_DEP_NAME-$ADD_DEP_VERSION.tgz" -o /dev/null
	fi
	;;
esac

if [ "${BENCH_HERMETIC:-0}" = "1" ]; then
	hermetic_use_no_uplink
fi

# ── Helper ─────────────────────────────────────────────────────────────────
#
# Each bench scenario is driven by:
#
#   - one shared `prepare_tpl` that sets the on-disk state for the
#     tool's project dir (wiping `node_modules`, dropping back the
#     saved lockfile, etc.)
#   - a per-tool command template looked up by `cmd_template`
#
# Template placeholders:
#   {project}       — project directory
#   {bin}           — tool binary
#   {home}          — isolated HOME directory
#   {store}         — store directory (pnpm/aube)
#   {cache}         — cache directory
#   {lockfile}      — saved lockfile path (source of the copy)
#   {lockfile_dest} — per-tool lockfile destination in the project
#                     directory (matches the pm's native filename)
#   {lockfile_b}    — saved branch-B lockfile path (branch-switch only;
#                     expands to an empty path for tools that never
#                     populated one)

expand_template() {
	local tpl=$1 project=$2 bin=$3 home=$4 store=$5 cache=$6 lockfile=$7 lockfile_dest=$8 lockfile_b=${9:-}
	tpl="${tpl//\{project\}/$project}"
	tpl="${tpl//\{bin\}/$bin}"
	tpl="${tpl//\{home\}/$home}"
	tpl="${tpl//\{store\}/$store}"
	tpl="${tpl//\{cache\}/$cache}"
	tpl="${tpl//\{lockfile\}/$lockfile}"
	tpl="${tpl//\{lockfile_dest\}/$lockfile_dest}"
	tpl="${tpl//\{lockfile_b\}/$lockfile_b}"
	echo "$tpl"
}

# Minimum publish-age gate, in minutes. aube defaults to 1440 (24h)
# as a supply-chain mitigation — the resolver skips versions newer
# than this window. The default forces aube to fetch the full
# (non-corgi) packument format so it can read the per-version `time`
# map; corgi omits `time` on npmjs.org. Most modern PMs support an
# equivalent flag, each with their own unit:
#
#   aube  minimumReleaseAge          (minutes; default 1440)
#   npm   --min-release-age          (days)
#   pnpm  --config.minimum-release-age (minutes)
#   bun   --minimum-release-age      (seconds)
#   deno  --minimum-dependency-age   (minutes, marked Unstable)
#   yarn  not supported
#   vlt   not investigated (currently disabled in BENCH_TOOLS anyway)
#
# Pinning all supported PMs to the same value makes the bench an
# apples-to-apples comparison — otherwise aube alone pays the
# full-packument cost (5x larger response on @types/node and similar
# heavily-versioned packuments) while bun/pnpm cruise on corgi.
#
# Override via `BENCH_MIN_RELEASE_AGE_MINUTES=0` to disable the gate
# across all PMs (useful for measuring raw resolver speed without
# the security-feature axis), or `default` to pin nothing anywhere —
# the T1 out-of-box posture: aube/nub keep their compiled-in 1440 and
# pay the full-packument cost their own default forces, everyone else
# cruises on corgi. The per-tool *_MRA_* fragments below expand to
# nothing in that mode.
MIN_RELEASE_AGE_MINUTES="${BENCH_MIN_RELEASE_AGE_MINUTES:-1440}"
export BENCH_MIN_RELEASE_AGE_MINUTES="$MIN_RELEASE_AGE_MINUTES"
if [ "$MIN_RELEASE_AGE_MINUTES" = "default" ]; then
	AUBE_MRA_ENV=""
	NPM_MRA_FLAG=""
	PNPM_MRA_FLAG=""
	BUN_MRA_FLAG=""
	DENO_MRA_FLAG=""
else
	MIN_RELEASE_AGE_SECONDS=$((MIN_RELEASE_AGE_MINUTES * 60))
	# npm uses days as the unit. Round up so the gate is at least as
	# strict as aube's, never weaker. (60*24 = 1440 → 1 day exactly.)
	MIN_RELEASE_AGE_DAYS=$(((MIN_RELEASE_AGE_MINUTES + 60 * 24 - 1) / (60 * 24)))
	AUBE_MRA_ENV="npm_config_minimum_release_age=${MIN_RELEASE_AGE_MINUTES}"
	NPM_MRA_FLAG="--min-release-age=${MIN_RELEASE_AGE_DAYS}"
	PNPM_MRA_FLAG="--config.minimum-release-age=${MIN_RELEASE_AGE_MINUTES}"
	BUN_MRA_FLAG="--minimum-release-age=${MIN_RELEASE_AGE_SECONDS}"
	DENO_MRA_FLAG="--minimum-dependency-age=${MIN_RELEASE_AGE_MINUTES}"
fi

# Global-virtual-store axis (BENCH_GVS, see header). FAST_GVS_ENV is the
# npm_config alias consumed by aube and nub; PNPM_GVS_FLAG is pnpm's
# spelling of the same setting (enableGlobalVirtualStore, experimental
# since pnpm 10.12). `pin-fast` reproduces upstream's published config:
# the fast engines pinned on so GitHub Actions' inherited CI=true can't
# silently flip them to per-project mode, pnpm at its default (off).
case "$BENCH_GVS" in
pin-fast)
	FAST_GVS_ENV="npm_config_enable_global_virtual_store=true"
	PNPM_GVS_FLAG=""
	;;
on)
	FAST_GVS_ENV="npm_config_enable_global_virtual_store=true"
	PNPM_GVS_FLAG="--config.enable-global-virtual-store=true"
	;;
off)
	FAST_GVS_ENV="npm_config_enable_global_virtual_store=false"
	PNPM_GVS_FLAG="--config.enable-global-virtual-store=false"
	;;
auto)
	FAST_GVS_ENV=""
	PNPM_GVS_FLAG=""
	;;
*)
	echo "error: BENCH_GVS must be pin-fast, on, off, or auto (got: $BENCH_GVS)" >&2
	exit 1
	;;
esac

# Advisory-check axis (BENCH_ADVISORY_CHECK, see header). Only aube/nub
# carry the OSV MAL-* gate; `off` is the T2 same-work pin (pnpm has no
# OSV check, npm's audit is already disabled via --no-audit).
case "$BENCH_ADVISORY_CHECK" in
default)
	FAST_ADVISORY_ENV=""
	;;
off)
	FAST_ADVISORY_ENV="npm_config_advisory_check=off"
	;;
*)
	echo "error: BENCH_ADVISORY_CHECK must be default or off (got: $BENCH_ADVISORY_CHECK)" >&2
	exit 1
	;;
esac

# Per-tool boilerplate factored out of the `CMDS` declarations below.
# Every bun invocation threads the same hermetic environment
# (isolated `HOME`, `BUN_INSTALL`, `--cache-dir`, `--ignore-scripts`,
# `--no-summary`) so the scenarios only have to spell out the
# install-mode flags that actually vary per scenario.
#
# `--minimum-release-age` is bun's name for the same supply-chain
# gate aube defaults on; matching the value here keeps the bench
# from advantaging bun by silently skipping work aube does.
BUN_BASE="HOME={home} BUN_INSTALL={home}/.bun {bin} install --cache-dir {cache} --ignore-scripts --no-summary ${BUN_MRA_FLAG}"

# aube reads the global store root from `$XDG_DATA_HOME/aube/store`
# (falling back to `$HOME/.local/share/aube/store`). We must pin
# `XDG_DATA_HOME` alongside `HOME` and `XDG_CACHE_HOME` — otherwise
# a host that already has `XDG_DATA_HOME` set in its environment
# would leak the benchmark's store out of the isolated `{home}`,
# and `COLD_WIPE` wouldn't find it to clean up between iterations.
# `npm_config_minimum_release_age` propagates the bench's
# minimum-release-age value into aube (aube reads this env var via
# its npm-compatible settings layer). Without it, aube uses its
# compiled-in default (1440) regardless of
# `BENCH_MIN_RELEASE_AGE_MINUTES`, silently breaking the
# apples-to-apples guarantee for any non-default override.
AUBE_ENV="HOME={home} XDG_CACHE_HOME={cache} XDG_DATA_HOME={home}/.local/share ${AUBE_MRA_ENV} ${FAST_ADVISORY_ENV}"

# Per-scenario AUBE_ENV variant carrying the GVS axis resolved from
# BENCH_GVS via the `enableGlobalVirtualStore` setting's auto-synthesized
# env-var alias (`npm_config_<snake_case>` — see `aube-settings/build.rs`).
# Using an env var rather than `--enable-gvs` means scenarios that go
# through `aube test` (which triggers auto-install internally) get the
# same forcing as direct `aube install` calls. The setting wins over
# `Linker::new`'s `CI` heuristic. With BENCH_GVS=auto the fragment is
# empty and the heuristic decides (CI is scrubbed, so on a dev-shaped
# env that resolves to GVS-on).
AUBE_ENV_GVS="$AUBE_ENV ${FAST_GVS_ENV}"

# nub embeds the aube engine, including its settings layer, so the same
# npm_config_* aliases (minimum-release-age, advisoryCheck,
# enableGlobalVirtualStore) steer it. Isolation mirrors AUBE_ENV: engine
# cache at $XDG_CACHE_HOME/nub/pm, CAS store at $XDG_DATA_HOME/nub/store.
NUB_ENV="HOME={home} XDG_CACHE_HOME={cache} XDG_DATA_HOME={home}/.local/share ${AUBE_MRA_ENV} ${FAST_ADVISORY_ENV}"
NUB_ENV_GVS="$NUB_ENV ${FAST_GVS_ENV}"

# Scenario keys describe what's on disk before the run. Every install
# scenario assumes a committed lockfile is present; the axes are
# cache/store warmth. The "install-test" scenario measures install +
# script dispatch end-to-end.

cmd_template() {
	case "$1:$2" in
	gvs-warm:aube | gvs-cold:aube)
		echo "cd {project} && $AUBE_ENV_GVS {bin} install --frozen-lockfile >/dev/null 2>&1"
		;;
	gvs-warm:nub | gvs-cold:nub)
		echo "cd {project} && $NUB_ENV_GVS {bin} install --frozen-lockfile >/dev/null 2>&1"
		;;
	gvs-warm:bun | gvs-cold:bun)
		echo "cd {project} && $BUN_BASE --frozen-lockfile >/dev/null 2>&1"
		;;
	gvs-warm:npm)
		echo "cd {project} && HOME={home} npm_config_cache={cache} {bin} ci --ignore-scripts --no-audit --no-fund --legacy-peer-deps --prefer-offline ${NPM_MRA_FLAG} >/dev/null 2>&1"
		;;
	gvs-warm:pnpm | gvs-cold:pnpm)
		echo "cd {project} && HOME={home} {bin} install --frozen-lockfile --ignore-scripts ${PNPM_MRA_FLAG} ${PNPM_GVS_FLAG} >/dev/null 2>&1"
		;;
	gvs-warm:yarn | gvs-cold:yarn)
		# Yarn 4: --immutable replaces --frozen-lockfile and aborts
		# if the lockfile or cache would change. Scripts/cache/linker
		# settings are already pinned in .yarnrc.yml.
		echo "cd {project} && HOME={home} {bin} install --immutable >/dev/null 2>&1"
		;;
	gvs-warm:deno | gvs-cold:deno)
		# Deno 2: --frozen errors out if the lockfile would change,
		# the equivalent of --frozen-lockfile elsewhere. Lifecycle
		# scripts are off unless --allow-scripts is passed.
		# `--minimum-dependency-age` is flagged "Unstable" in deno's
		# help but the flag itself parses fine; takes minutes.
		echo "cd {project} && HOME={home} DENO_DIR={cache} {bin} install --frozen --quiet ${DENO_MRA_FLAG} >/dev/null 2>&1"
		;;
	gvs-warm:vlt | gvs-cold:vlt)
		# vlt's --frozen-lockfile mirrors pnpm/npm/aube semantics: refuse
		# to re-resolve and error out if vlt-lock.json would change.
		# Without it vlt would silently treat the install as a fresh
		# resolve, which is not what the other tools measure here.
		echo "cd {project} && HOME={home} npm_config_cache={cache} {bin} install --frozen-lockfile >/dev/null 2>&1"
		;;
	gvs-cold:npm)
		echo "cd {project} && HOME={home} npm_config_cache={cache} {bin} ci --ignore-scripts --no-audit --no-fund --legacy-peer-deps ${NPM_MRA_FLAG} >/dev/null 2>&1"
		;;
	install-test:aube)
		echo "cd {project} && $AUBE_ENV_GVS {bin} test >/dev/null 2>&1"
		;;
	install-test:nub)
		# nub has no `test` verb that auto-installs; the equivalent
		# developer loop is `nub install` (state-hash short-circuit via
		# the embedded engine) followed by `nub run test`. nub discovers
		# Node from $PATH, so the isolated HOME doesn't trigger Node
		# provisioning here.
		echo "cd {project} && $NUB_ENV_GVS {bin} install >/dev/null 2>&1 && $NUB_ENV_GVS {bin} run test >/dev/null 2>&1"
		;;
	install-test:bun)
		echo "cd {project} && $BUN_BASE --frozen-lockfile >/dev/null 2>&1 && HOME={home} BUN_INSTALL={home}/.bun {bin} run test >/dev/null 2>&1"
		;;
	install-test:npm)
		echo "cd {project} && HOME={home} npm_config_cache={cache} {bin} install-test --ignore-scripts --no-audit --no-fund --legacy-peer-deps --prefer-offline ${NPM_MRA_FLAG} >/dev/null 2>&1"
		;;
	install-test:pnpm)
		echo "cd {project} && HOME={home} {bin} install-test --frozen-lockfile --ignore-scripts ${PNPM_MRA_FLAG} ${PNPM_GVS_FLAG} >/dev/null 2>&1"
		;;
	install-test:yarn)
		echo "cd {project} && HOME={home} {bin} install --immutable >/dev/null 2>&1 && HOME={home} {bin} test >/dev/null 2>&1"
		;;
	install-test:deno)
		echo "cd {project} && HOME={home} DENO_DIR={cache} {bin} install --frozen --quiet ${DENO_MRA_FLAG} >/dev/null 2>&1 && HOME={home} DENO_DIR={cache} {bin} task --quiet test >/dev/null 2>&1"
		;;
	install-test:vlt)
		echo "cd {project} && HOME={home} npm_config_cache={cache} {bin} install --frozen-lockfile >/dev/null 2>&1 && HOME={home} npm_config_cache={cache} {bin} run test >/dev/null 2>&1"
		;;

	# ── ci-loop (S10): the warm CI loop, measured out-of-box ──────────
	# Same shape as install-test, with one deliberate difference: NO GVS
	# pin on aube/nub (plain $AUBE_ENV/$NUB_ENV, not the _GVS variants)
	# and no pnpm GVS flag. The scenario exists to expose what each tool
	# does on its own under CI=true vs CI unset — aube's Linker heuristic
	# flips its global virtual store off under CI, and pinning it would
	# erase exactly the behavior being measured. BENCH_GVS does not apply
	# here by design.
	ci-loop:aube)
		echo "cd {project} && $AUBE_ENV {bin} test >/dev/null 2>&1"
		;;
	ci-loop:nub)
		echo "cd {project} && $NUB_ENV {bin} install >/dev/null 2>&1 && $NUB_ENV {bin} run test >/dev/null 2>&1"
		;;
	ci-loop:bun)
		echo "cd {project} && $BUN_BASE --frozen-lockfile >/dev/null 2>&1 && HOME={home} BUN_INSTALL={home}/.bun {bin} run test >/dev/null 2>&1"
		;;
	ci-loop:npm)
		echo "cd {project} && HOME={home} npm_config_cache={cache} {bin} install-test --ignore-scripts --no-audit --no-fund --legacy-peer-deps --prefer-offline ${NPM_MRA_FLAG} >/dev/null 2>&1"
		;;
	ci-loop:pnpm)
		echo "cd {project} && HOME={home} {bin} install-test --frozen-lockfile --ignore-scripts ${PNPM_MRA_FLAG} >/dev/null 2>&1"
		;;
	ci-loop:yarn)
		# Note: .yarnrc.yml pins enableImmutableInstalls=false (needed
		# for populate), so yarn's CI=true row is not fully stock — the
		# immutable-install flip is suppressed. Recorded here so nobody
		# reads the yarn CI delta as out-of-box.
		echo "cd {project} && HOME={home} {bin} install --immutable >/dev/null 2>&1 && HOME={home} {bin} test >/dev/null 2>&1"
		;;
	ci-loop:deno)
		echo "cd {project} && HOME={home} DENO_DIR={cache} {bin} install --frozen --quiet ${DENO_MRA_FLAG} >/dev/null 2>&1 && HOME={home} DENO_DIR={cache} {bin} task --quiet test >/dev/null 2>&1"
		;;
	ci-loop:vlt)
		echo "cd {project} && HOME={home} npm_config_cache={cache} {bin} install --frozen-lockfile >/dev/null 2>&1 && HOME={home} npm_config_cache={cache} {bin} run test >/dev/null 2>&1"
		;;

	# ── dev-install: the timed command for add-dep and branch-switch ──
	# The developer-loop spelling of `install`: no --frozen-lockfile, so
	# the tool may re-resolve when package.json/lockfile drifted (add-dep
	# forces that; branch-switch hands it a valid lockfile B and measures
	# the node_modules diff). GVS / release-age / advisory knobs apply as
	# resolved above.
	dev-install:aube)
		echo "cd {project} && $AUBE_ENV_GVS {bin} install >/dev/null 2>&1"
		;;
	dev-install:nub)
		echo "cd {project} && $NUB_ENV_GVS {bin} install >/dev/null 2>&1"
		;;
	dev-install:bun)
		echo "cd {project} && $BUN_BASE >/dev/null 2>&1"
		;;
	dev-install:npm)
		echo "cd {project} && HOME={home} npm_config_cache={cache} {bin} install --ignore-scripts --no-audit --no-fund --legacy-peer-deps --prefer-offline ${NPM_MRA_FLAG} >/dev/null 2>&1"
		;;
	dev-install:pnpm)
		echo "cd {project} && HOME={home} {bin} install --ignore-scripts ${PNPM_MRA_FLAG} ${PNPM_GVS_FLAG} >/dev/null 2>&1"
		;;
	dev-install:yarn)
		echo "cd {project} && HOME={home} {bin} install >/dev/null 2>&1"
		;;
	dev-install:deno)
		echo "cd {project} && HOME={home} DENO_DIR={cache} {bin} install --quiet ${DENO_MRA_FLAG} >/dev/null 2>&1"
		;;
	dev-install:vlt)
		echo "cd {project} && HOME={home} npm_config_cache={cache} {bin} install >/dev/null 2>&1"
		;;
	esac
}

run_bench() {
	local bench_name=$1
	local prepare_tpl=$2

	for i in "${!TOOLS[@]}"; do
		local tool="${TOOLS[$i]}"
		local project="${TOOL_PROJECTS[$i]}"
		local bin="${TOOL_BINS[$i]}"
		local home="${TOOL_HOMES[$i]}"
		local store="${TOOL_STORES[$i]}"
		local cache="${TOOL_CACHES[$i]}"
		local lockfile="$BENCH_DIR/saved-lockfile-$tool"
		local lockfile_dest
		lockfile_dest="$project/$(lockfile_name_for "$tool")"

		local cmd_tpl
		cmd_tpl=$(cmd_template "$bench_name" "$tool")
		if [ -z "$cmd_tpl" ]; then
			echo "warning: no $bench_name command for $tool — skipping" >&2
			continue
		fi

		local prepare
		prepare=$(expand_template "$prepare_tpl" "$project" "$bin" "$home" "$store" "$cache" "$lockfile" "$lockfile_dest")

		local cmd
		cmd=$(expand_template "$cmd_tpl" "$project" "$bin" "$home" "$store" "$cache" "$lockfile" "$lockfile_dest")

		local tool_runs
		tool_runs=$(runs_for_tool "$tool")
		echo ""
		echo "  $tool:"
		hyperfine \
			--warmup "$WARMUP" \
			--runs "$tool_runs" \
			--ignore-failure \
			--prepare "$prepare" \
			--command-name "$tool" \
			"$cmd" \
			--export-json "$BENCH_DIR/${bench_name}-${tool}.json" ||
			true
	done
}

# Like `run_bench`, but times the *second* invocation of the tool's
# command — the prepare step wipes node_modules, restores the saved
# lockfile, and runs the same command once so the timed iteration
# starts from a "node_modules is already valid" state.
#
# Used by the install-test scenario to measure the "I've installed,
# now I just want to re-run my tests" developer loop rather than the
# "fresh checkout + install" loop (which `gvs-warm` already covers).
run_bench_preinstall() {
	local bench_name=$1

	for i in "${!TOOLS[@]}"; do
		local tool="${TOOLS[$i]}"
		local project="${TOOL_PROJECTS[$i]}"
		local bin="${TOOL_BINS[$i]}"
		local home="${TOOL_HOMES[$i]}"
		local store="${TOOL_STORES[$i]}"
		local cache="${TOOL_CACHES[$i]}"
		local lockfile="$BENCH_DIR/saved-lockfile-$tool"
		local lockfile_dest
		lockfile_dest="$project/$(lockfile_name_for "$tool")"

		local cmd_tpl
		cmd_tpl=$(cmd_template "$bench_name" "$tool")
		if [ -z "$cmd_tpl" ]; then
			echo "warning: no $bench_name command for $tool — skipping" >&2
			continue
		fi

		local cmd
		cmd=$(expand_template "$cmd_tpl" "$project" "$bin" "$home" "$store" "$cache" "$lockfile" "$lockfile_dest")

		local warm_prep
		warm_prep=$(expand_template "$WARM_PREP" "$project" "$bin" "$home" "$store" "$cache" "$lockfile" "$lockfile_dest")

		# Prepare: wipe + restore lockfile, then run the same command
		# once untimed so the tool's install phase populates
		# node_modules (and `.aube-state` for aube). The timed
		# iteration then re-runs the command against the settled
		# state — the developer-loop "run my tests again" case.
		local prepare="$warm_prep && $cmd"

		local tool_runs
		tool_runs=$(runs_for_tool "$tool")
		echo ""
		echo "  $tool:"
		hyperfine \
			--warmup "$WARMUP" \
			--runs "$tool_runs" \
			--ignore-failure \
			--prepare "$prepare" \
			--command-name "$tool" \
			"$cmd" \
			--export-json "$BENCH_DIR/${bench_name}-${tool}.json" ||
			true
	done
}

# Generalized "restore A → settle → mutate → time" runner for the
# staged scenarios (ci-loop, add-dep, branch-switch).
#
#   $1 timed_key       — cmd_template key for the TIMED command
#   $2 settle_key      — cmd_template key run once untimed in prepare,
#                        after restoring package.json A + lockfile A +
#                        wiping node_modules, so the timed run starts
#                        from a settled, declared state
#   $3 result_name     — hyperfine JSON basename (also the results row
#                        key; may differ from the scenario key, e.g.
#                        ci-loop → ci-loop-ci / ci-loop-noci)
#   $4 post_settle_tpl — optional template appended to prepare AFTER the
#                        settle run (the mutation: inject a dep, swap in
#                        the B fixture, …). Empty = no mutation.
#   $5 ci_mode         — "ci" to run hyperfine (and thus every prepare +
#                        timed command) with CI=true; default is the
#                        scrubbed environment (CI unset globally above).
run_bench_staged() {
	local timed_key=$1 settle_key=$2 result_name=$3 post_settle_tpl=$4 ci_mode=${5:-}

	for i in "${!TOOLS[@]}"; do
		local tool="${TOOLS[$i]}"
		local project="${TOOL_PROJECTS[$i]}"
		local bin="${TOOL_BINS[$i]}"
		local home="${TOOL_HOMES[$i]}"
		local store="${TOOL_STORES[$i]}"
		local cache="${TOOL_CACHES[$i]}"
		local lockfile="$BENCH_DIR/saved-lockfile-$tool"
		local lockfile_b="$BENCH_DIR/saved-lockfile-b-$tool"
		local lockfile_dest
		lockfile_dest="$project/$(lockfile_name_for "$tool")"

		local timed_tpl settle_tpl
		timed_tpl=$(cmd_template "$timed_key" "$tool")
		settle_tpl=$(cmd_template "$settle_key" "$tool")
		if [ -z "$timed_tpl" ] || [ -z "$settle_tpl" ]; then
			echo "warning: no $result_name command for $tool — skipping" >&2
			continue
		fi

		local cmd settle warm_prep
		cmd=$(expand_template "$timed_tpl" "$project" "$bin" "$home" "$store" "$cache" "$lockfile" "$lockfile_dest" "$lockfile_b")
		settle=$(expand_template "$settle_tpl" "$project" "$bin" "$home" "$store" "$cache" "$lockfile" "$lockfile_dest" "$lockfile_b")
		warm_prep=$(expand_template "$WARM_PREP" "$project" "$bin" "$home" "$store" "$cache" "$lockfile" "$lockfile_dest" "$lockfile_b")

		# Manifest state A first (staged scenarios mutate package.json,
		# including workspace-member ones), then the lockfile-A restore
		# + node_modules wipe, then the settle run, then the mutation.
		local prepare="cp -R $FIXTURE_A_OVERLAY/. $project/ && $warm_prep && $settle"
		if [ -n "$post_settle_tpl" ]; then
			local post_settle
			post_settle=$(expand_template "$post_settle_tpl" "$project" "$bin" "$home" "$store" "$cache" "$lockfile" "$lockfile_dest" "$lockfile_b")
			prepare="$prepare && $post_settle"
		fi

		# CI is a recorded matrix variable: scrubbed everywhere, set
		# only for the ci-loop-ci row. hyperfine's children (prepare +
		# timed command) inherit its environment.
		local -a env_prefix=(env -u CI)
		if [ "$ci_mode" = "ci" ]; then
			env_prefix=(env CI=true)
		fi

		local tool_runs
		tool_runs=$(runs_for_tool "$tool")
		echo ""
		echo "  $tool:"
		"${env_prefix[@]}" hyperfine \
			--warmup "$WARMUP" \
			--runs "$tool_runs" \
			--ignore-failure \
			--prepare "$prepare" \
			--command-name "$tool" \
			"$cmd" \
			--export-json "$BENCH_DIR/${result_name}-${tool}.json" ||
			true
	done
}

PHASES_FILE="$BENCH_DIR/aube-install-phases.jsonl"
: >"$PHASES_FILE"

run_aube_phase_bench() {
	local bench_name=$1
	local prepare_tpl=$2

	for i in "${!TOOLS[@]}"; do
		local tool="${TOOLS[$i]}"
		[ "$tool" = "aube" ] || continue

		local project="${TOOL_PROJECTS[$i]}"
		local bin="${TOOL_BINS[$i]}"
		local home="${TOOL_HOMES[$i]}"
		local store="${TOOL_STORES[$i]}"
		local cache="${TOOL_CACHES[$i]}"
		local lockfile="$BENCH_DIR/saved-lockfile-$tool"
		local lockfile_dest
		lockfile_dest="$project/$(lockfile_name_for "$tool")"

		local prepare
		prepare=$(expand_template "$prepare_tpl" "$project" "$bin" "$home" "$store" "$cache" "$lockfile" "$lockfile_dest")

		local cmd_tpl
		cmd_tpl=$(cmd_template "$bench_name" "$tool")
		local cmd
		cmd=$(expand_template "$cmd_tpl" "$project" "$bin" "$home" "$store" "$cache" "$lockfile" "$lockfile_dest")
		# Inject phase-timing env vars after the `cd {project} && `
		# prefix. We can't use `${cmd/&& /&& ...}` here: bash 5.2+
		# treats `&` in the replacement of `${var/pat/repl}` as a
		# backreference to the matched pattern (sed-like), so each
		# unescaped `&` expands to the matched `&& ` and the result
		# becomes `cd <p> && && ...` which fails at eval time.
		# Escaping with `\&` works on 5.2+ but emits literal backslashes
		# on bash 3.2 (macOS dev shell). Splitting around `&& ` avoids
		# both traps and works the same on every bash we care about.
		cmd="${cmd%%&& *}&& AUBE_BENCH_PHASES_FILE=$PHASES_FILE AUBE_BENCH_SCENARIO=$bench_name ${cmd#*&& }"

		echo "  $bench_name"
		if ! eval "$prepare"; then
			echo "warning: phase timing prepare failed for $bench_name - skipping sample" >&2
			continue
		fi
		if ! eval "$cmd"; then
			echo "warning: phase timing run failed for $bench_name - skipping sample" >&2
			continue
		fi
	done
}

# Directories to wipe in cold scenarios. Each pm has its own cache /
# store layout, so we reset everything we know about to guarantee
# a fresh download on every iteration.
COLD_WIPE='{store} {cache} {home}/.pnpm-store {home}/.local/share/aube {home}/.local/share/nub {home}/.npm {home}/.yarn {home}/.bun {home}/.cache/aube {home}/.cache/nub {home}/.cache/yarn {home}/.cache/bun {home}/.cache/deno {home}/.cache/vlt {home}/.config/vlt {home}/Library/Caches/deno'

# Warm-cache lockfile restore: wipe the project-local state (lockfile
# + node_modules) and drop the saved lockfile back. Uses the per-tool
# `lockfile_dest` placeholder so each pm gets its native filename.
# `{project}/packages/*/node_modules` covers workspace fixtures' member
# node_modules; on the single-package fixture the glob doesn't match and
# `rm -rf` ignores the literal path.
WARM_PREP="rm -rf {project}/node_modules {project}/packages/*/node_modules {project}/pnpm-lock.yaml {project}/aube-lock.yaml {project}/package-lock.json {project}/yarn.lock {project}/bun.lock {project}/bun.lockb {project}/deno.lock {project}/vlt-lock.json && cp {lockfile} {lockfile_dest}"
COLD_PREP="rm -rf {project}/node_modules {project}/packages/*/node_modules {project}/pnpm-lock.yaml {project}/aube-lock.yaml {project}/package-lock.json {project}/yarn.lock {project}/bun.lock {project}/bun.lockb {project}/deno.lock {project}/vlt-lock.json $COLD_WIPE && mkdir -p {home} && cp {lockfile} {lockfile_dest}"

# ── Benchmark 1: Fresh install, warm cache ─────────────────────────────────
# Lockfile present, node_modules deleted, store and cache warm.
# Pins aube's default local global virtual store behavior so GitHub
# Actions' inherited CI=true environment cannot silently turn this into
# per-project mode.

echo ""
echo "━━━ Benchmark 1: Fresh install (warm cache) ━━━"
run_scenario "gvs-warm" run_bench "gvs-warm" "$WARM_PREP"

# ── Benchmark 2: Fresh install, cold cache ─────────────────────────────────
# Lockfile present, but store and cache are empty.
# Measures fetch-from-registry + import + link/materialization work.

echo ""
echo "━━━ Benchmark 2: Fresh install (cold cache) ━━━"
run_scenario "gvs-cold" run_bench "gvs-cold" "$COLD_PREP"

# ── Aube phase timing sample ───────────────────────────────────────────────
# Hyperfine owns stdout/stderr and times whole commands. For attribution,
# run aube once per install-shaped scenario with AUBE_BENCH_PHASES_FILE
# enabled so the binary writes structured resolve/fetch/link/script/state
# timings to JSONL, then summarize it at the end.

echo ""
echo "━━━ Aube install phase timings ━━━"
if [ "$BENCH_PHASES" != "0" ]; then
	run_scenario "gvs-warm" run_aube_phase_bench "gvs-warm" "$WARM_PREP"
	run_scenario "gvs-cold" run_aube_phase_bench "gvs-cold" "$COLD_PREP"
fi

# ── Benchmark 3: install + run test (developer loop) ───────────────────────
# Warm store+cache, lockfile present, node_modules *already* populated.
# Models the developer-loop case: "I've installed, now I keep re-running
# my tests." Each iteration's prepare runs the full install-test command
# once (untimed) so node_modules and any tool-specific state files are
# valid, then the timed iteration re-runs the same command. Tools with a
# state-based short-circuit (aube's .aube-state) skip install entirely
# on the timed run; tools without one still pay for lockfile revalidation.

echo ""
echo "━━━ Benchmark 3: install + run test (already installed) ━━━"
run_scenario "install-test" run_bench_preinstall "install-test"

# ── Benchmark 4: warm CI loop, CI unset vs CI=true (S10) ──────────────────
# install-test's developer loop, re-measured as an explicitly-labeled CI
# story: two rows, identical on-disk state, the only delta is the CI env
# var. Tools are run out-of-box w.r.t. their store heuristics (no GVS
# pin — see the ci-loop cmd_template comment), so the row pair shows
# exactly what a real CI user gets vs what a dev-machine user gets.

echo ""
echo "━━━ Benchmark 4: warm CI loop (CI unset) ━━━"
run_scenario "ci-loop" run_bench_staged "ci-loop" "ci-loop" "ci-loop-noci" ""

echo ""
echo "━━━ Benchmark 4b: warm CI loop (CI=true) ━━━"
run_scenario "ci-loop" run_bench_staged "ci-loop" "ci-loop" "ci-loop-ci" "" ci

# ── Benchmark 5: add one dep (S11) ─────────────────────────────────────────
# Warm everything, settle a frozen install, inject one pinned dep into
# package.json, time the non-frozen install. This is the resolution/
# packument path under each tool's resolved config — note aube/nub's
# advisory check (when not pinned off) fires on exactly this flow.

echo ""
echo "━━━ Benchmark 5: add one dep (${ADD_DEP_NAME}@${ADD_DEP_VERSION}) ━━━"
run_scenario "add-dep" run_bench_staged "dev-install" "gvs-warm" "add-dep" \
	"node $SCRIPT_DIR/add-dep.mjs {project}/package.json $ADD_DEP_NAME $ADD_DEP_VERSION"

# ── Benchmark 6: branch switch (S12) ───────────────────────────────────────
# node_modules settled and valid for lockfile A; swap in package.json B
# + the tool's native lockfile B (~15 version deltas, generated during
# populate); time the incremental install. The most common real dev
# operation — and the cell where diff-based installers shine vs
# all-or-nothing state checks.

echo ""
echo "━━━ Benchmark 6: branch switch (~15 version deltas) ━━━"
run_scenario "branch-switch" run_bench_staged "dev-install" "gvs-warm" "branch-switch" \
	"cp -R $FIXTURE_B_OVERLAY/. {project}/ && cp {lockfile_b} {lockfile_dest}"

# ── Summary ────────────────────────────────────────────────────────────────

RESULTS_MD="$BENCH_DIR/results.md"

echo ""
echo "━━━ Results ━━━"
TOOLS_CSV=$(
	IFS=,
	echo "${TOOLS[*]}"
)
BENCH_TOOLS="$TOOLS_CSV" BENCH_SCENARIOS="$BENCH_SCENARIOS" RUNS="$RUNS" WARMUP="$WARMUP" node "$SCRIPT_DIR/generate-results.js" "$BENCH_DIR" "$RESULTS_MD"
if [ -s "$PHASES_FILE" ]; then
	echo ""
	node "$SCRIPT_DIR/generate-phase-results.mjs" "$PHASES_FILE" "$BENCH_DIR/aube-install-phases.md"
fi
echo ""
echo "Results saved to: $RESULTS_MD"
echo ""
echo "Temp directory kept at: $BENCH_DIR"
echo "Remove with: rm -rf $BENCH_DIR"
