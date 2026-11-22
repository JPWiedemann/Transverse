// Tests for release.mjs. Run with: node --test scripts/
import assert from "node:assert/strict"
import { execFileSync } from "node:child_process"
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { test } from "node:test"
import { fileURLToPath } from "node:url"

import { applyBump, bumpLevel, parseCommit, parseVersion, renderNotes } from "./release.mjs"

const script = fileURLToPath(new URL("./release.mjs", import.meta.url))

test("parses conventional commit titles", () => {
  assert.deepEqual(parseCommit("feat(gui): add a tab (#12)"), {
    type: "feat",
    scope: "gui",
    description: "add a tab",
    pr: "12",
    breaking: false,
  })
  assert.equal(parseCommit("fix: bug").scope, undefined)
  assert.equal(parseCommit("feat(core)!: change the API").breaking, true)
  assert.equal(parseCommit("refactor: x", "BREAKING CHANGE: old files stop working").breaking, true)
  assert.equal(parseCommit("Merge branch 'dev' into main"), null)
  assert.equal(parseCommit("fixed stuff"), null)
  assert.equal(parseCommit("feat:no space"), null)
  assert.equal(parseCommit("FEAT: shouting").type, "feat")
})

test("version bump rules differ below and above 1.0", () => {
  const c = (type, breaking = false) => ({ type, breaking })
  assert.equal(bumpLevel("0.3.0", [c("feat")]), "minor")
  assert.equal(bumpLevel("0.3.0", [c("fix")]), "patch")
  assert.equal(bumpLevel("0.3.0", [c("perf")]), "patch")
  assert.equal(bumpLevel("0.3.0", [c("fix"), c("feat", true)]), "minor", "breaking below 1.0 is minor")
  assert.equal(bumpLevel("1.2.0", [c("fix"), c("feat", true)]), "major")
  assert.equal(bumpLevel("1.2.0", [c("feat"), c("fix")]), "minor")
  for (const t of ["docs", "refactor", "test", "build", "ci", "chore"]) {
    assert.equal(bumpLevel("1.0.0", [c(t)]), null, t)
  }
  assert.equal(bumpLevel("1.0.0", []), null)
  assert.equal(applyBump("0.3.4", "minor"), "0.4.0")
  assert.equal(applyBump("0.3.4", "patch"), "0.3.5")
  assert.equal(applyBump("1.3.4", "major"), "2.0.0")
  assert.throws(() => parseVersion("1.2"), /not a MAJOR/)
  assert.throws(() => parseVersion("1.2.3-rc1"))
})

test("release notes are grouped and omit commits that do not release", () => {
  const notes = renderNotes(
    [
      parseCommit("feat(gui): field tab (#3)"),
      parseCommit("fix: readout unit"),
      parseCommit("chore: bump"),
      parseCommit("perf(core): faster solve"),
      parseCommit("feat(core)!: new format"),
    ].filter(Boolean),
  )
  assert.match(notes, /### ⚠ Breaking changes\n\n- \*\*feat\(core\):\*\* new format/)
  assert.match(notes, /### Features\n\n- \*\*gui:\*\* field tab \(#3\)/)
  assert.match(notes, /### Bug fixes\n\n- readout unit/)
  assert.match(notes, /### Performance\n\n- \*\*core:\*\* faster solve/)
  assert.doesNotMatch(notes, /bump/)
  assert.equal(notes.split("new format").length - 1, 1, "a breaking change appears only in its own section")
})

// ---- the script on real (temporary) repositories ---------------------------------------------

function repo() {
  const dir = mkdtempSync(join(tmpdir(), "release-test-"))
  const git = (...a) => execFileSync("git", a, { cwd: dir, encoding: "utf8" }).trim()
  git("init", "-q", "-b", "main")
  git("config", "user.email", "test@example.com")
  git("config", "user.name", "Test")
  git("config", "commit.gpgsign", "false")
  let n = 0
  const commit = (subject) => {
    writeFileSync(join(dir, "f.txt"), String(++n))
    git("add", "f.txt")
    git("commit", "-q", "-m", subject)
  }
  const run = (...args) => {
    try {
      return JSON.parse(execFileSync("node", [script, ...args], { cwd: dir, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }))
    } catch (e) {
      return { error: String(e.stderr) }
    }
  }
  return { dir, git, commit, run }
}

test("first release without any tag counts all commits from 0.0.0", () => {
  const r = repo()
  r.commit("chore: setup")
  assert.equal(r.run().release, false)
  r.commit("feat: first feature")
  r.commit("fix: something")
  const out = r.run()
  assert.equal(out.version, "0.1.0")
  assert.equal(out.tag, "v0.1.0")
  assert.equal(out.since, "the beginning")
  assert.equal(out.commits, 3)
})

test("next releases start at the latest tag", () => {
  const r = repo()
  r.commit("feat: first")
  r.git("tag", "v0.1.0")
  r.commit("docs: readme")
  assert.equal(r.run().release, false)
  r.commit("fix: a bug (#4)")
  assert.equal(r.run().version, "0.1.1")
  r.commit("feat: a feature")
  assert.equal(r.run().version, "0.2.0")
  r.git("tag", "v0.10.0") // numeric, not alphabetical order: 0.10.0 is newer than 0.9.x
  r.git("tag", "v0.9.5", "HEAD~1")
  r.commit("fix: after 0.10.0")
  const out = r.run()
  assert.equal(out.since, "v0.10.0")
  assert.equal(out.version, "0.10.1")
  assert.equal(out.commits, 1)
})

test("squash-merged dev history and merge commits on main", () => {
  const r = repo()
  r.commit("feat: initial")
  r.git("checkout", "-q", "-b", "dev")
  r.commit("feat: one (#1)")
  r.commit("fix: two (#2)")
  // release: merge commit on main, tagged
  r.git("checkout", "-q", "main")
  r.git("merge", "--no-ff", "-q", "-m", "chore(release): next release", "dev")
  r.git("tag", "v0.1.0")
  // dev continues without a back-merge
  r.git("checkout", "-q", "dev")
  r.commit("fix: three (#3)")
  const out = r.run("--ref", "dev")
  assert.equal(out.since, "v0.1.0")
  assert.equal(out.commits, 1, "only what dev gained since the tagged merge")
  assert.equal(out.version, "0.1.1")
})

test("--release-as overrides but must be greater than the last release", () => {
  const r = repo()
  r.commit("feat: first")
  r.git("tag", "v0.4.0")
  r.commit("fix: x")
  assert.deepEqual([r.run("--release-as", "1.0.0").version, r.run("--release-as", "1.0.0").bump], ["1.0.0", "override"])
  assert.match(r.run("--release-as", "0.3.0").error, /must be greater/)
  assert.match(r.run("--release-as", "0.4.0").error, /must be greater/)
})

test("preview text and notes file", () => {
  const r = repo()
  r.commit("feat(gui): shiny (#7)")
  const file = join(r.dir, "notes.md")
  r.run("--preview", "--notes-file", file)
  const notes = readFileSync(file, "utf8")
  assert.match(notes, /^\*\*Next release: v0\.1\.0\*\* \(minor\), changes since the beginning:/)
  assert.match(notes, /- \*\*gui:\*\* shiny \(#7\)/)
  const r2 = repo()
  r2.commit("ci: only ci")
  const file2 = join(r2.dir, "n.md")
  r2.run("--preview", "--notes-file", file2)
  assert.match(readFileSync(file2, "utf8"), /^\*\*No release yet\.\*\*/)
})
