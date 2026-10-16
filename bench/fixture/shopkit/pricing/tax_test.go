package pricing

import "testing"

func TestWithTax(t *testing.T) {
	cases := []struct{ in, want int64 }{{0, 0}, {100, 120}, {1999, 2399}, {5, 6}}
	for _, c := range cases {
		if got := WithTax(c.in); got != c.want {
			t.Errorf("WithTax(%d) = %d, want %d", c.in, got, c.want)
		}
	}
}
