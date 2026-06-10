// Reads the per-scenario hyperfine JSON output from `bench.sh` and
// emits two artifacts:
//
//   1. A human-readable markdown summary at `outputFile` (same format
//      the console prints).
//   2. A structured JSON file at `<outputFile without extension>.json`
//      so downstream consumers — notably `docs/benchmarks.data.ts`,
//      the VitePress data loader behind the `<BenchChart>` on the
//      benchmarks page — can ingest the results without parsing
//      markdown.
//
// Usage:
//   node generate-results.js <benchDir> <outputMarkdown>
// Optional env:
//   BENCH_TOOLS=aube,bun,pnpm,npm,yarn,deno,vlt
//                                   comma-separated tool order
//                                   (defaults to aube + pnpm)
//   RESULTS_JSON=<path>             override the JSON output path

const fs = require('fs')
const path = require('path')

const benchDir = process.argv[2]
const outputFile = process.argv[3]

const benchmarks = [
  ['gvs-warm', 'Fresh install (warm cache)'],
  ['gvs-cold', 'Fresh install (cold cache)'],
  ['install-test', 'npm install && npm run test'],
  // S10: same on-disk state, the CI env var is the only delta between
  // the two rows. Labeled as what it measures — an install
  // short-circuit plus script dispatch — not as a generic "install".
  ['ci-loop-noci', 'Warm CI loop, CI unset (install short-circuit + test)'],
  ['ci-loop-ci', 'Warm CI loop, CI=true (install short-circuit + test)'],
  // S11 / S12: the resolution-path and incremental-install cells.
  ['add-dep', 'Add one dep (package.json edit + non-frozen install)'],
  ['branch-switch', 'Branch switch (~15 lockfile deltas, warm node_modules)'],
]
const SELECTED_BENCHMARKS = new Set(
  (process.env.BENCH_SCENARIOS || benchmarks.map(([name]) => name).join(','))
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean),
)
// bench.sh spells S10 as one scenario key (`ci-loop`) but emits two
// result rows; expand the selection so both land in the output.
if (SELECTED_BENCHMARKS.has('ci-loop')) {
  SELECTED_BENCHMARKS.add('ci-loop-noci')
  SELECTED_BENCHMARKS.add('ci-loop-ci')
}

const TOOLS = (process.env.BENCH_TOOLS || 'aube,pnpm')
  .split(',')
  .map((s) => s.trim())
  .filter(Boolean)

function readResult (benchDir, name, tool) {
  try {
    const data = JSON.parse(fs.readFileSync(`${benchDir}/${name}-${tool}.json`, 'utf8'))
    const r = data.results[0]
    if (!r || !Number.isFinite(r.mean)) {
      throw new Error('missing benchmark mean')
    }
    const stddev = Number.isFinite(r.stddev) ? r.stddev : 0
    // Median is the headline statistic: min overstates best-case (the
    // pnpm.io approach), mean is noise-sensitive on small N. Mean/min/max
    // stay in `stats` so the raw shape is never lost.
    const median = Number.isFinite(r.median) ? r.median : r.mean
    return {
      text: `${median.toFixed(3)}s ± ${stddev.toFixed(3)}s`,
      median,
      mean: r.mean,
      stddev,
      min: r.min,
      max: r.max,
    }
  } catch (err) {
    if (err && err.code !== 'ENOENT') {
      console.error(`Warning: failed to read ${name}-${tool}: ${err.message}`)
    }
    return { text: 'n/a', median: null, mean: null, stddev: null, min: null, max: null }
  }
}

// Ratios are computed from medians (same statistic as the headline cells).
function fmtSpeedup (baseMedian, heroMedian) {
  if (baseMedian == null || heroMedian == null) return ''
  if (heroMedian < baseMedian) {
    return ` (${(baseMedian / heroMedian).toFixed(1)}x faster)`
  } else if (heroMedian > baseMedian) {
    return ` (${(heroMedian / baseMedian).toFixed(1)}x slower)`
  }
  return ''
}

// -- Markdown ---------------------------------------------------------------
// Emits one row per scenario with a column per tool plus trailing
// "vs pnpm" and "vs bun" speedup columns when those tools are present
// in the run. pnpm is aube's drop-in-replacement target; bun is the
// other "fast" package manager users compare against. When nub (the
// Rust CLI embedding the aube engine) runs without aube, it takes the
// hero seat; when both run, an extra "nub vs aube" column surfaces the
// fork overhead (the two should be ~equal — divergence is a regression).
const HERO = TOOLS.includes('aube') ? 'aube' : (TOOLS.includes('nub') ? 'nub' : null)
const headerCells = ['#', 'Scenario', ...TOOLS]
if (HERO && TOOLS.includes('pnpm')) {
  headerCells.push('vs pnpm')
}
if (HERO && TOOLS.includes('bun')) {
  headerCells.push('vs bun')
}
if (TOOLS.includes('nub') && TOOLS.includes('aube')) {
  headerCells.push('nub vs aube')
}

const lines = [
  '# Benchmark Results',
  '',
  `| ${headerCells.join(' | ')} |`,
  `|${headerCells.map(() => '---').join('|')}|`,
]

// -- Structured JSON --------------------------------------------------------
// bench.sh writes BENCH_VERSIONS_FILE as a "<tool>\t<semver>" TSV so
// the docs chart can render the actual version each manager was
// running rather than just the bare name.
const versions = {}
const versionsFile = process.env.BENCH_VERSIONS_FILE
if (versionsFile && fs.existsSync(versionsFile)) {
  for (const line of fs.readFileSync(versionsFile, 'utf8').split('\n')) {
    const [name, version] = line.split('\t')
    if (name && version) versions[name] = version.trim()
  }
}
// `nub --version` prints only the nub version; the embedded aube engine
// rev comes from the vendored submodule, which only the runner knows.
if (process.env.BENCH_NUB_ENGINE_VERSION) {
  versions['nub-aube-engine'] = process.env.BENCH_NUB_ENGINE_VERSION
}

// The environment block makes every results.json self-describing: which
// config tier / GVS cell / advisory + release-age pins produced these
// numbers. No number is publishable without its tier label, so the
// label rides with the data. bench.sh exports the resolved knob values.
const environment = {
  tier: process.env.BENCH_TIER || null,
  gvs: process.env.BENCH_GVS || 'pin-fast',
  advisoryCheck: process.env.BENCH_ADVISORY_CHECK || 'default',
  minimumReleaseAgeMinutes: process.env.BENCH_MIN_RELEASE_AGE_MINUTES || '1440',
  ci: 'scrubbed (set only in the ci-loop-ci scenario)',
  runs: process.env.RUNS || null,
  warmup: process.env.WARMUP || null,
  hermetic: process.env.BENCH_HERMETIC === '1',
  bandwidth: process.env.BENCH_BANDWIDTH || null,
  latency: process.env.BENCH_LATENCY || null,
  fixture: process.env.BENCH_FIXTURE || 'default',
}

const json = {
  updated: new Date().toISOString(),
  unit: 'ms',
  managers: TOOLS,
  versions,
  environment,
  rows: [],
}

benchmarks.filter(([name]) => SELECTED_BENCHMARKS.has(name)).forEach(([name, label], i) => {
  const results = {}
  for (const tool of TOOLS) {
    results[tool] = readResult(benchDir, name, tool)
  }

  const cells = [String(i + 1), label]
  for (const tool of TOOLS) {
    cells.push(results[tool].text)
  }
  if (HERO && TOOLS.includes('pnpm')) {
    cells.push(fmtSpeedup(results.pnpm.median, results[HERO].median).trim())
  }
  if (HERO && TOOLS.includes('bun')) {
    cells.push(fmtSpeedup(results.bun.median, results[HERO].median).trim())
  }
  if (TOOLS.includes('nub') && TOOLS.includes('aube')) {
    cells.push(fmtSpeedup(results.aube.median, results.nub.median).trim())
  }
  lines.push(`| ${cells.join(' | ')} |`)

  // `values` carries the headline statistic (median, ms); the full
  // mean/median/stddev/min/max shape lives in `stats`.
  const values = {}
  const stats = {}
  for (const tool of TOOLS) {
    values[tool] = results[tool].median == null ? null : Math.round(results[tool].median * 1000)
    stats[tool] = results[tool].median == null ? null : results[tool]
  }

  json.rows.push({ key: name, label, values, stats })
})

lines.push('')

const output = lines.join('\n')
fs.writeFileSync(outputFile, output)
console.log(output)

const jsonOut = process.env.RESULTS_JSON
  || `${outputFile.replace(/\.md$/, '')}.json`
fs.writeFileSync(jsonOut, JSON.stringify(json, null, 2) + '\n')
console.log(`Wrote structured results to ${path.resolve(jsonOut)}`)
