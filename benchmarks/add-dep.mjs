// Inject one pinned dependency into a project's package.json.
//
// Used by bench.sh's `add-dep` scenario: the timed command is a plain
// (non-frozen) `<pm> install` after this script adds the dep, so every
// tool does identical work — resolve one new package against an
// otherwise-settled project. We deliberately do NOT use each tool's
// `add` verb: the verbs bundle different extra work per tool (lockfile
// rewrite strategy, audit hooks, output), and nub has no `add` verb at
// all. The package.json edit is the common denominator.
//
// Usage: node add-dep.mjs <path/to/package.json> <name> <exact-version>
import { readFileSync, writeFileSync } from 'node:fs'

const [, , pkgPath, name, version] = process.argv
if (!pkgPath || !name || !version) {
  console.error('usage: node add-dep.mjs <package.json> <name> <version>')
  process.exit(1)
}

const pkg = JSON.parse(readFileSync(pkgPath, 'utf8'))
pkg.dependencies = pkg.dependencies || {}
pkg.dependencies[name] = version
writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n')
