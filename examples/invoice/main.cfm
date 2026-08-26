<cfscript>
// ---------------------------------------------------------------------------
// Both ways of making a document, backed by Typst.
//
//   rustcfml ext build .. && rustcfml ext install ../typst-0.1.0.rcx --dir extensions
//   rustcfml main.cfm
//
// Note what is NOT here: a single '#'. Typst markup is built out of '#', and '#'
// is CFML's interpolation character, so raw Typst in a CFML string has to double
// every one (##set, ##table). The builder emits the markup for you, so
// application code never types one — and never has to think about escaping,
// because every value you pass becomes a Typst string literal, not Typst code.
// ---------------------------------------------------------------------------

writeOutput( "Typst " & typstVersion() & ", " & typstFontCount() & " font faces visible" & chr(10) );

lines = queryNew(
      "sku,item,qty,unitPrice,amount"
    , "varchar,varchar,integer,decimal,decimal"
    , [ [ "W-1", "Widget, large",  3, 10,  30  ]
      , [ "G-2", "Gadget",         1, 99,  99  ]
      , [ "S-3", "Sprocket set",  12, 12, 144  ] ]
);

// ---------------------------------------------------------------------------
// 1. The fluent builder
// ---------------------------------------------------------------------------

doc = Document()
    .title( "Invoice INV-1042" )
    .author( "Acme Ltd" )
    .keywords( "invoice,acme,INV-1042" )
    .pageSize( "a4" )
    .margin( top = 2, bottom = 2, left = 1.8, right = 1.8, unit = "cm" )
    .font( family = "Helvetica", size = 11 )
    .header( content = "Acme Ltd", align = "right" )
    .footer( content = "Page {page} of {total}", align = "center" )
    .headingNumbering( "1." )                     // needed to .ref() a heading

    .heading( text = "Invoice INV-1042", level = 1, label = "invoice" )
    .paragraph( text = "26 August 2026", color = "gray" )
    .rule()
    .spacer( amount = 0.5, unit = "cm" );

// A grid — a stroke-less table — is how you lay out two blocks side by side.
doc.grid(
      data   = [ [ "Acme Ltd" & chr(10) & "1 Example Way" & chr(10) & "London", "Due: 25 September 2026" & chr(10) & "Terms: Net 30" ] ]
    , widths = "1,1"
    , unit   = "fr"
);

// Rich inline text: the whole paragraph is no longer bold-or-not.
doc.paragraph( text = [
      "Thanks for your business. Please pay "
    , { text = "273.00", bold = true }
    , " by "
    , { text = "25 September", underline = true }
    , ", per "
    , { ref = "terms" }
    , { text = ".", footnote = "Interest accrues at 8% above base from the day after." }
] );

// The table: a query in, a formatted table out. columnList selects and orders,
// columnFormats carries the number mask — applied by the ENGINE's numberFormat,
// so it matches every other formatted number in the application.
doc.table(
      data          = lines
    , columnList    = "item,qty,unitPrice,amount"
    , headers       = "Description,Qty,Unit,Amount"
    , widths        = "3,1,1,1"
    , unit          = "fr"
    , columnFormats = {
          qty       = { alignment = "center" }
        , unitPrice = { alignment = "right", numberFormat = "9,999.00" }
        , amount    = { alignment = "right", numberFormat = "9,999.00" }
      }
    , headerFormat  = { bold = true, bgcolor = "##2c3e50", color = "white" }
    , stripe        = "##f7f7f7"
    , stroke        = false
    , totals        = "amount"
    , totalsLabel   = "Total due"
    , caption       = "Charges"
    , label         = "charges"
);

doc.spacer( amount = 0.6, unit = "cm" )
   .callout(
         content = [ "Pay online at ", { link = "https://acme.example/pay/INV-1042", text = "acme.example/pay" }, " or by transfer to the account on file." ]
       , fill    = "##fff8e1"
       , stroke  = "##ffe082"
       , radius  = 3
       , inset   = 10
       , unit    = "pt"
   );

doc.pageBreak()
   .heading( text = "Terms", level = 2, label = "terms" )
   .terms( items = {
         "Net 30"  = "Payable within 30 days of the invoice date."
       , "Title"   = "Goods remain our property until paid for in full."
       , "Late"    = "Interest at 8% above base rate."
   } )
   .quote( text = "Cash is a fact, profit is an opinion.", attribution = "Alfred Rappaport" );

// Injection: a value that IS Typst code renders as text, not as code.
doc.paragraph( "Reference: ##set page(fill: black) [not code]" );

// Whitelists are refusals, not passthroughs.
try {
    doc.spacer( amount = 1, unit = "furlongs" );
} catch ( any e ) {
    writeOutput( "unit whitelist: " & e.message & chr(10) );
}
try {
    doc.paragraph( text = [ { link = "javascript:alert(1)", text = "click" } ] );
} catch ( any e ) {
    writeOutput( "link whitelist: " & e.message & chr(10) );
}

invoicePdf = doc.toBinary();
writeOutput( "invoice: " & doc.pageCount() & " pages, " & arrayLen( invoicePdf ) & " bytes" & chr(10) );

out = getTempDirectory() & "/typst_document_invoice.pdf";
doc.write( path = out, overwrite = true );

// ...and straight back in through the engine's own PDF support (v0.634.0).
back = pdfRead( out );
writeOutput( "read back: " & pdfPageCount( back ) & " pages, page 1 is "
           & pdfInfo( back ).pagesizes[ 1 ].width & "x" & pdfInfo( back ).pagesizes[ 1 ].height & "pt" & chr(10) );

// ---------------------------------------------------------------------------
// 2. A template — the designer owns the layout, CFML supplies the data
// ---------------------------------------------------------------------------

statement = Document()
    .template( path = expandPath( "./templates/statement.typ" ) )
    .data( {
          reference = "STMT-2026-0007"
        , date      = "26 August 2026"
        , customer  = { name = "Acme Ltd", address = "1 Example Way, London" }
        , lines     = [ { description = "Widget, large", qty = 3, amount = 30 }
                      , { description = "Gadget",        qty = 1, amount = 99 } ]
    } );

statementOut = getTempDirectory() & "/typst_document_statement.pdf";
statement.write( path = statementOut, overwrite = true );
writeOutput( "statement: " & statement.pageCount() & " page from a template" & chr(10) );

// The template's own root is its directory, so it could #import _shared.typ.
// It could NOT have read anything above that directory.
writeOutput( "wrote " & out & chr(10) & "wrote " & statementOut & chr(10) );

// The debugging seam: exactly what was compiled, for either shape.
writeOutput( chr(10) & "--- generated Typst (invoice) ---" & chr(10) & doc.toTypst() );
</cfscript>
