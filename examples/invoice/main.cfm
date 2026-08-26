<cfscript>
// ---------------------------------------------------------------------------
// The fluent Document() builder, backed by Typst, running inside a bespoke
// binary built by `rustcfml --build .`.
//
// Note what is NOT here: a single '#'. Typst markup is built out of '#' and '#'
// is CFML's interpolation character, so raw Typst in a CFML string has to
// double every one (##set, ##table). The builder emits the markup for you, so
// application code never types one — and never has to think about escaping,
// because every value you pass becomes a Typst string literal, not Typst code.
// ---------------------------------------------------------------------------

writeOutput( "font faces visible to the engine: " & typstFontCount() & chr(10) );

lines = queryNew(
      "item,qty,price"
    , "varchar,integer,decimal"
    , [ [ "Widget, large",  3, "30.00" ]
      , [ "Gadget",         1, "99.00" ]
      , [ "Sprocket set",  12, "144.00" ] ]
);

doc = Document()
    .title( "Invoice INV-1042" )
    .author( "Acme Ltd" )
    .pageSize( "a4" )
    .margin( top = 2, bottom = 2, left = 1.8, right = 1.8, unit = "cm" )
    .font( family = "Helvetica", size = 11 )
    .header( content = "Acme Ltd", align = "right" )
    .footer( content = "Page {page} of {total}", align = "center" )

    .heading( text = "Invoice INV-1042", level = 1 )
    .paragraph( text = "26 August 2026", color = "gray" )
    .rule()
    .spacer( amount = 0.5, unit = "cm" )

    .paragraph( "Thanks for your business. Payment is due within 30 days." )
    .table(
          data    = lines
        , header  = true
        , align   = "left,right,right"
        , columns = "1,1,1"
        , unit    = "fr"
    );

// Units are a whitelist, not a passthrough — nothing the caller types ever
// becomes a Typst literal.
try {
    doc.spacer( amount = 1, unit = "furlongs" );
} catch ( any e ) {
    writeOutput( "unit whitelist: " & e.message & chr(10) );
}

doc.spacer( amount = 1, unit = "cm" )
   .paragraph( text = "Total due: 273.00", bold = true, size = 13, align = "right" )
   .pageBreak()
   .heading( text = "Terms", level = 2 )
   .list( [ "Payment within 30 days.", "Late payment attracts interest at 8%.", "Goods remain our property until paid for." ] );

// Injection: a value that IS Typst code must render as text, not run.
doc.paragraph( "Reference: ##set page(fill: black) [not code]" );

pdf = doc.toBinary();
writeOutput( "pages: " & doc.pageCount() & chr(10) );
writeOutput( "pdf bytes: " & arrayLen( pdf ) & chr(10) );

out = getTempDirectory() & "/typst_document_invoice.pdf";
doc.write( path = out, overwrite = true );

// ...and straight back in through the engine's own PDF support (v0.634.0).
read = pdfRead( out );
writeOutput( "read back: " & pdfPageCount( read ) & " pages, page 1 is "
           & pdfInfo( read ).pagesizes[ 1 ].width & "x" & pdfInfo( read ).pagesizes[ 1 ].height & "pt" & chr(10) );
writeOutput( "wrote " & out & chr(10) );

// The debugging seam: see exactly what was compiled.
writeOutput( chr(10) & "--- generated Typst ---" & chr(10) & doc.toTypst() );
</cfscript>
