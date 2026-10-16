package md

import (
	"strings"
	"testing"
)

func TestCodeRefs(t *testing.T) {
	cases := []struct {
		line string
		want []string
	}{
		{"Open with WAL in `internal/db/open.go` and see ./cmd/migrate/main.go:42", []string{"internal/db/open.go", "cmd/migrate/main.go"}},
		{`panic at C:\work\shop\payments\refund.go:31 +0x1f`, []string{"c:/work/shop/payments/refund.go"}},
		{"Migrations live in migrations/ and run via cmd/migrate", []string{"migrations/"}},
		{"Past the window, payments.Refund returns payments.ErrRefundWindow; call audit.Record", []string{"pkg:payments", "pkg:audit"}},
		{"See https://example.com/docs/api.json for the schema", nil},
		{"e.g. This is prose. Version 1.2.3 and a ratio of 3/4.", nil},
	}
	for _, c := range cases {
		got := CodeRefs(c.line)
		if strings.Join(got, ",") != strings.Join(c.want, ",") {
			t.Errorf("%q\n got  %v\n want %v", c.line, got, c.want)
		}
	}
}

func TestDocCodeRefs(t *testing.T) {
	d := Parse([]byte("---\nsources: [internal/db/open.go]\n---\n# X\n\n## Files\n- payments/refund.go\n\n```\ngoroutine 1:\n  cart/cart.go:41\n```\n"))
	var refs []string
	for _, r := range d.CodeRefs {
		refs = append(refs, r.Ref)
	}
	got := strings.Join(refs, ",")
	for _, want := range []string{"internal/db/open.go", "payments/refund.go", "cart/cart.go"} {
		if !strings.Contains(got, want) {
			t.Errorf("missing %s in %s", want, got)
		}
	}
}
