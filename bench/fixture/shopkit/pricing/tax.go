// Package pricing holds price arithmetic. Amounts are int64 kuruş.
package pricing

// TaxRateBP is the VAT rate in basis points (20% = 2000).
const TaxRateBP = 2000

// WithTax adds VAT to a net amount, rounding half up.
func WithTax(net int64) int64 {
	return net + (net*TaxRateBP+5000)/10000
}
