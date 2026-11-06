package pricing

import "testing"

func TestHidden_WithTaxGermany(t *testing.T) {
	cases := []struct{ in, want int64 }{{0, 0}, {100, 119}, {5, 6}, {2500, 2975}}
	for _, c := range cases {
		if got := WithTax(c.in, "DE"); got != c.want {
			t.Errorf("WithTax(%d, DE) = %d, want %d", c.in, got, c.want)
		}
	}
}

func TestHidden_WithTaxElsewhere(t *testing.T) {
	for _, country := range []string{"FR", "TR", ""} {
		if got := WithTax(100, country); got != 120 {
			t.Errorf("WithTax(100, %q) = %d, want 120", country, got)
		}
	}
}
