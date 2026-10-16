# Windows checkout with core.autocrlf=true: the catalog arrives with CRLF line endings.
sed -i 's/$/\r/' catalog/testdata/catalog.json
