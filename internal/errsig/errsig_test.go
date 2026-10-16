package errsig

import "testing"

func TestNormalize(t *testing.T) {
	cases := []struct{ a, b string }{
		{`catalog_test.go:11: catalog checksum mismatch: got 3f2a9c1e0b7d want 4ff52f25fc9d`,
			`catalog_test.go:42: catalog checksum mismatch: got 0011aa22bb33 want 4ff52f25fc9d`},
		{`open C:\Users\ali\proj\testdata\golden/x.json: The system cannot find the file`,
			`open /home/ci/work/proj/testdata/golden/x.json: The system cannot find the file`},
		{`dial tcp 127.0.0.1:6379: connect: connection refused`, `dial tcp 10.0.0.12:6380: connect: connection refused`},
		{`database is locked (5)`, `database is locked (517)`},
		{`2026-10-14T10:00:01Z ERROR request 550e8400-e29b-41d4-a716-446655440000 failed`, `2026-11-02T22:13:59Z ERROR request 6ba7b810-9dad-11d1-80b4-00c04fd430c8 failed`},
		{`panic: runtime error: index out of range [5] with length 3`, `panic: runtime error: index out of range [9] with length 2`},
		{`undefined: "fooBar"`, `undefined: "otherName"`},
	}
	for _, c := range cases {
		if Normalize(c.a) != Normalize(c.b) {
			t.Errorf("not equal:\n %q\n %q", Normalize(c.a), Normalize(c.b))
		}
	}
	// different errors stay different
	if Normalize("connection refused") == Normalize("connection reset") {
		t.Error("distinct errors collapsed")
	}
	if got := Normalize(`catalog/catalog.go:31:2: undefined: bytes`); got != "catalog.go<loc>: undefined: bytes" {
		t.Errorf("got %q", got)
	}
}

func TestLines(t *testing.T) {
	out := "=== RUN   TestLoad\n    catalog_test.go:9: catalog checksum mismatch: got 3f2a9c1e0b7d want 4ff52f25fc9d\n--- FAIL: TestLoad (0.00s)\nFAIL\nFAIL\texample.com/shopkit/catalog\t0.412s\nok  \texample.com/shopkit/cart\t0.3s\n"
	ls := Lines(out, 5)
	if len(ls) == 0 || ls[0].Norm != "catalog_test.go<loc>: catalog checksum mismatch: got <hex> want <hex>" {
		t.Fatalf("%+v", ls)
	}
	for _, l := range ls {
		if l.Raw == "FAIL" || l.Raw == "FAIL\texample.com/shopkit/catalog\t0.412s" {
			t.Errorf("noise line kept: %q", l.Raw)
		}
	}
}
