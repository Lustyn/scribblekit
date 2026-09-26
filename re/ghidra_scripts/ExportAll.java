// Decompile every function and dump C, plus strings with xrefs, for grep-based RE.
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import ghidra.program.model.data.*;
import ghidra.program.model.address.*;
import java.io.*;

public class ExportAll extends GhidraScript {
    @Override
    public void run() throws Exception {
        String outDir = getScriptArgs()[0];
        DecompInterface ifc = new DecompInterface();
        ifc.openProgram(currentProgram);
        try (PrintWriter w = new PrintWriter(new FileWriter(outDir + "/decompiled.c"))) {
            FunctionIterator it = currentProgram.getFunctionManager().getFunctions(true);
            while (it.hasNext() && !monitor.isCancelled()) {
                Function f = it.next();
                DecompileResults r = ifc.decompileFunction(f, 60, monitor);
                w.println("// ==== " + f.getName() + " @ " + f.getEntryPoint());
                if (r != null && r.decompileCompleted()) w.println(r.getDecompiledFunction().getC());
                else w.println("// decompile failed");
            }
        }
        try (PrintWriter w = new PrintWriter(new FileWriter(outDir + "/strings.txt"))) {
            DataIterator di = currentProgram.getListing().getDefinedData(true);
            ReferenceManager rm = currentProgram.getReferenceManager();
            while (di.hasNext()) {
                Data d = di.next();
                if (!(d.getDataType() instanceof AbstractStringDataType)) continue;
                StringBuilder refs = new StringBuilder();
                for (Reference ref : rm.getReferencesTo(d.getAddress())) {
                    Function f = getFunctionContaining(ref.getFromAddress());
                    refs.append(" ").append(f != null ? f.getName() : ref.getFromAddress().toString());
                }
                w.println(d.getAddress() + "\t" + String.valueOf(d.getValue()).replace("\n", "\\n") + "\t<-" + refs);
            }
        }
    }
}
