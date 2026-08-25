<cfscript>
// Proof that Typst compiles from inside a native module, and that the PDF it
// produces is readable by the engine's own PDF support.
//
// NOTE THE DOUBLED HASHES. Typst markup is built out of '#' (##set, ##table,
// ##pagebreak) and '#' is CFML's interpolation character, so raw Typst in a
// CFML string must escape every one. This is the strongest ergonomic argument
// for the fluent Document() builder: it emits the Typst for you, so application
// code never types a '#' at all.

writeOutput( "font faces visible to the engine: " & typstFontCount() & chr(10) );

markup = '
##set page(paper: "a4", margin: 2cm, numbering: "1 / 1")
##set text(font: "Helvetica", size: 11pt)
= Invoice INV-1042
*Acme Ltd* — 26 August 2026

##table(
  columns: (1fr, auto, auto),
  table.header([*Item*], [*Qty*], [*Price*]),
  [Widget], [3], [30.00],
  [Gadget], [1], [99.00],
  [*Total*], [], [*129.00*],
)

##pagebreak()
== Terms
Payment due within 30 days.
';

writeOutput( "pages: " & typstPageCount( markup ) & chr(10) );

pdf = typstCompile( markup );
writeOutput( "pdf bytes: " & arrayLen( pdf ) & chr(10) );

out = getTempDirectory() & "/typst_module_invoice.pdf";
fileWrite( out, pdf );

// ...and straight back in through the engine's native PDF support (v0.634.0),
// which is a nice demonstration that the two halves meet.
doc = pdfRead( out );
writeOutput( "read back: " & pdfPageCount( doc ) & " pages, page 1 is "
           & pdfInfo( doc ).pagesizes[ 1 ].width & "x" & pdfInfo( doc ).pagesizes[ 1 ].height & "pt" & chr(10) );

png = getTempDirectory() & "/typst_module_invoice.png";
imageWrite( pdfToImage( doc, 1, 400 ), png );
writeOutput( "rasterised page 1 -> " & getFileInfo( png ).size & " byte png" & chr(10) );
writeOutput( "wrote " & out & chr(10) );
</cfscript>
