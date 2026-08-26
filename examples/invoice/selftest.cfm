<cfscript>
// Assertions over the Document() surface. Run it the same way as main.cfm:
//   rustcfml --build . && ./app --port 8829   →   /selftest.cfm
// Everything here compiles real Typst, so a pass means the markup was valid,
// not merely that it was generated.

failures = 0;
function check( required string label, required boolean ok, string detail = "" ) {
    if ( ok ) {
        writeOutput( "  ok    " & label & chr(10) );
    } else {
        failures++;
        writeOutput( "  FAIL  " & label & ( len( detail ) ? " — " & detail : "" ) & chr(10) );
    }
}

// ---- table sources ---------------------------------------------------------
q = queryNew( "name,qty", "varchar,integer", [ [ "Widget", 3 ], [ "Gadget", 1 ] ] );
fromQuery = Document().table( data = q ).toTypst();
check( "query table has a header row",   fromQuery contains "table.header" );
check( "query table carries its rows",   fromQuery contains "Widget" AND fromQuery contains "Gadget" );
check( "query table counts columns",     fromQuery contains "columns: 2" );

fromStructs = Document().table( data = [ { name = "A", qty = 1 }, { name = "B", qty = 2 } ] ).toTypst();
check( "array of structs uses the first row's keys", fromStructs contains "columns: 2" );

fromArrays = Document().table( data = [ [ "a", "b" ], [ "c" ] ], header = false ).toTypst();
check( "ragged array rows are padded", listLen( fromArrays, chr(10) ) GT 0 AND fromArrays contains "columns: 2" );

// ---- escaping --------------------------------------------------------------
hostile = "##set page(fill: black) $x^2$ ] [";
esc = Document().paragraph( hostile ).toTypst();
check( "hostile text is a string literal, not code", esc contains '"##set page(fill: black) $x^2$ ] ["' );
check( "hostile text compiles",                      arrayLen( Document().paragraph( hostile ).toBinary() ) GT 0 );

// ---- whitelists ------------------------------------------------------------
badUnit = false;
try { Document().spacer( amount = 1, unit = "furlongs" ); } catch ( any e ) { badUnit = true; }
check( "an unknown unit is rejected", badUnit );

badColor = false;
try { Document().paragraph( text = "x", color = "chartreuse" ); } catch ( any e ) { badColor = true; }
check( "an unknown colour is rejected", badColor );

badLevel = false;
try { Document().heading( text = "x", level = 9 ); } catch ( any e ) { badLevel = true; }
check( "a heading level outside 1-6 is rejected", badLevel );

badMethod = false;
try { Document().nope(); } catch ( any e ) { badMethod = true; }
check( "an unknown method is rejected", badMethod );

// ---- named-argument binding (engine v0.635.0+) -----------------------------
reordered = Document().heading( level = 3, text = "Reordered" ).toTypst();
check( "named args bind by name, not position", reordered contains '##heading(level: 3)[##"Reordered"]' );

badArg = false;
try { Document().heading( txt = "x" ); } catch ( any e ) { badArg = true; }
check( "an unknown argument name is rejected", badArg );

// ---- images ----------------------------------------------------------------
png = imageNew( "", 40, 20, "rgb", "red" );
tmp = getTempDirectory() & "/rustcfml_typst_selftest.png";
imageWrite( png, tmp );

withPath = Document().image( source = tmp, width = 40, unit = "pt" );
check( "an image from a path compiles", arrayLen( withPath.toBinary() ) GT 0 );

withBinary = Document().image( source = fileReadBinary( tmp ), width = 40, unit = "pt" );
check( "an image from binary compiles", arrayLen( withBinary.toBinary() ) GT 0 );

notAnImage = false;
try { Document().image( source = "not/an/image.png" ); } catch ( any e ) { notAnImage = true; }
check( "a missing image file is an error", notAnImage );

// A document can NOT reach the filesystem itself, even via raw Typst.
escaped = false;
try { Document().typst( '##image("/etc/hosts")' ).toBinary(); } catch ( any e ) { escaped = true; }
check( "raw Typst cannot open a path of its own", escaped );

// ---- structure -------------------------------------------------------------
twoPages = Document().paragraph( "one" ).pageBreak().paragraph( "two" );
check( "pageBreak produces a second page", twoPages.pageCount() EQ 2 );

lists = Document().list( items = [ "a", "b" ], ordered = true ).toTypst();
check( "ordered list uses enum", lists contains "##enum(" );

landscape = Document().orientation( "landscape" ).toTypst();
check( "landscape flips the page", landscape contains "flipped: true" );

counters = Document().footer( content = "Page {page} of {total}" ).paragraph( "x" ).toTypst();
check( "footer placeholders become counters", counters contains "counter(page).display()" );

// ---- terminals -------------------------------------------------------------
out = getTempDirectory() & "/rustcfml_typst_selftest.pdf";
Document().paragraph( "written" ).write( path = out, overwrite = true );
check( "write() produced a PDF", fileExists( out ) AND getFileInfo( out ).size GT 0 );

refused = false;
try { Document().paragraph( "x" ).write( path = out, overwrite = false ); } catch ( any e ) { refused = true; }
check( "write( overwrite=false ) refuses an existing file", refused );

check( "isDocumentObject recognises a document", isDocumentObject( Document() ) );
check( "isDocumentObject rejects a string",      NOT isDocumentObject( "nope" ) );

writeOutput( chr(10) & ( failures ? "FAILURES: " & failures : "all checks passed" ) & chr(10) );
</cfscript>
