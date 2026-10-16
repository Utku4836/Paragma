package md

import (
	"regexp"
	"strings"
)

// CodeRef is a mention of source code in a note: a file path, a directory
// ("migrations/") or a package-qualified identifier ("payments.Refund",
// stored as "pkg:payments"). They connect memory to the code an agent is
// about to touch.
type CodeRef struct {
	Ref     string // normalised: lower case, forward slashes, no leading ./
	Line    int
	Section int
}

const codeExt = `go|ts|tsx|js|jsx|mjs|cjs|py|rs|java|kt|rb|cs|php|c|cc|cpp|h|hpp|swift|sql|ya?ml|json|toml|sh|ps1|proto|tf|css|scss|html|vue|svelte|md`

var (
	reCodePath = regexp.MustCompile(`(?:^|[\s(\[{"'` + "`" + `,=])((?:[A-Za-z]:)?(?:\.{0,2}[\\/])?(?:[\w.@-]+[\\/])*[\w.@-]+\.(?:` + codeExt + `))(?::\d+)*\b`)
	reCodeDir  = regexp.MustCompile(`(?:^|[\s(\["'` + "`" + `])((?:[\w.@-]+/)+)(?:[\s)\]"'` + "`" + `,;:]|$)`)
	rePkgRef   = regexp.MustCompile(`\b([a-z][a-z0-9_]{1,30})\.([A-Z][A-Za-z0-9_]*)\b`)
	reBarePath = regexp.MustCompile(`(?:^|[\s(\["'` + "`" + `])([a-z_][\w.-]*(?:/[\w.-]+)+/?)(?:[\s)\]"'` + "`" + `,;:.]|$)`)
)

// NormRef normalises a code path for comparison.
func NormRef(p string) string {
	p = strings.ToLower(strings.ReplaceAll(strings.TrimSpace(p), "\\", "/"))
	for strings.HasPrefix(p, "./") {
		p = p[2:]
	}
	return p
}

// pkgStop are lower-case words that often precede ".Capital" in prose but
// are not packages.
var pkgStop = map[string]bool{"e": true, "eg": true, "ie": true, "vs": true, "etc": true, "fig": true, "www": true, "com": true, "org": true, "io": true}

// CodeRefs extracts code references from a line.
func CodeRefs(line string) []string {
	var out []string
	seen := map[string]bool{}
	add := func(r string) {
		if r != "" && !seen[r] {
			seen[r] = true
			out = append(out, r)
		}
	}
	if strings.Contains(line, "://") {
		// drop URLs before matching so hosts and paths of links are not code
		line = regexp.MustCompile(`\w+://\S+`).ReplaceAllString(line, " ")
	}
	for _, m := range reCodePath.FindAllStringSubmatch(line, -1) {
		add(NormRef(m[1]))
	}
	for _, m := range reCodeDir.FindAllStringSubmatch(line, -1) {
		d := NormRef(m[1])
		if strings.ContainsAny(d, "abcdefghijklmnopqrstuvwxyz") && !strings.HasPrefix(d, "http") {
			add(d)
		}
	}
	// directories written without a trailing slash: `internal/db` in
	// backticks, or a bare path with two or more slashes (a/b/c)
	for _, m := range reBarePath.FindAllStringSubmatch(line, -1) {
		p := m[1]
		inTicks := strings.Contains(line, "`"+p+"`")
		if !inTicks && strings.Count(p, "/") < 2 {
			continue
		}
		if reCodePath.MatchString(" "+p) || strings.HasPrefix(p, "http") {
			continue // a file: already taken above
		}
		add(NormRef(strings.TrimSuffix(p, "/") + "/"))
	}
	for _, m := range rePkgRef.FindAllStringSubmatch(line, -1) {
		if !pkgStop[m[1]] {
			add("pkg:" + m[1])
		}
	}
	return out
}

// parseCodeRefs collects code references from the body (code blocks
// included: stack traces name files) and from a front matter `sources` list.
func (d *Doc) parseCodeRefs() {
	for _, s := range d.FrontList("sources") {
		for _, r := range CodeRefs(" " + s + " ") {
			d.CodeRefs = append(d.CodeRefs, CodeRef{Ref: r, Line: 1, Section: -1})
		}
		if !strings.ContainsAny(s, " \t") {
			if r := NormRef(s); r != "" && !strings.Contains(r, "pkg:") {
				d.CodeRefs = append(d.CodeRefs, CodeRef{Ref: r, Line: 1, Section: -1})
			}
		}
	}
	for i := d.BodyLine - 1; i < len(d.Lines); i++ {
		for _, r := range CodeRefs(d.Lines[i]) {
			d.CodeRefs = append(d.CodeRefs, CodeRef{Ref: r, Line: i + 1, Section: d.SectionAt(i + 1)})
		}
	}
}
