package addr

import "testing"

func TestParse(t *testing.T) {
	cases := []struct {
		in   string
		want Addr
	}{
		{"rules", Addr{File: "rules"}},
		{"rules#Veritabanı", Addr{File: "rules", Heading: "Veritabanı"}},
		{"architecture#Katmanlar/Depolama", Addr{File: "architecture", Heading: "Katmanlar/Depolama"}},
		{"notes#Devir[2]", Addr{File: "notes", Heading: "Devir", Nth: 2}},
		{"auth-refactor^t3f2", Addr{File: "auth-refactor", Block: "t3f2"}},
		{"^t3f2", Addr{Block: "t3f2"}},
		{"stack:10-24", Addr{File: "stack", From: 10, To: 24}},
		{"stack:10", Addr{File: "stack", From: 10, To: 10}},
		{"stack:10-", Addr{File: "stack", From: 10}},
		{"@01JA3Z8K2M", Addr{ULID: "01JA3Z8K2M"}},
		{"@01ja3z8k2m#Plan", Addr{ULID: "01JA3Z8K2M", Heading: "Plan"}},
		{"[[rules#Go|x]]", Addr{File: "rules", Heading: "Go"}},
		{"notes#Time: 10:30", Addr{File: "notes", Heading: "Time: 10:30"}},
		{"episodes/errors/x.md", Addr{File: "episodes/errors/x.md"}},
	}
	for _, c := range cases {
		got, err := Parse(c.in)
		if err != nil {
			t.Errorf("%q: %v", c.in, err)
			continue
		}
		got.Raw = ""
		if got != c.want {
			t.Errorf("%q: got %+v want %+v", c.in, got, c.want)
		}
	}
	for _, bad := range []string{"", "x:5-2", "x#", "x^"} {
		if _, err := Parse(bad); err == nil {
			t.Errorf("%q: expected error", bad)
		}
	}
}
