/** Include this to ensure that all functions are defined. */
import { _functions } from "./defineFunction.js";

const functions = _functions;
export default functions;

// TODO(kevinb): have functions return an object and call defineFunction with
// that object in this file instead of relying on side-effects.
import "./functions/accent.js";
import "./functions/accentunder.js";
import "./functions/arrow.js";
import "./functions/bordermatrix.js"
import "./functions/cancelto.js";
import "./environments/cd.js";
import "./functions/char.js";
import "./functions/color.js";
import "./functions/cr.js";
import "./functions/def.js";
import "./functions/delimiter.js";
import "./functions/enclose.js";
import "./functions/environment.js";
import "./functions/envTag.js";
import "./functions/font.js";
import "./functions/genfrac.js";
import "./functions/hbox.js";
import "./functions/horizBracket.js";
import "./functions/html.js";
import "./functions/includegraphics.js";
import "./functions/kern.js";
import "./functions/label.js";
import "./functions/lap.js";
import "./functions/math.js";
import "./functions/mathchoice.js";
import "./functions/mclass.js";
import "./functions/multiscript.js";
import "./functions/not.js";
import "./functions/op.js";
import "./functions/operatorname.js";
import "./functions/ordgroup.js";
import "./functions/phantom.js";
import "./functions/pmb.js";
import "./functions/raise.js";
import "./functions/ref.js";
import "./functions/reflect.js";
import "./functions/relax.js";
import "./functions/rule.js";
import "./functions/sfrac.js";
import "./functions/sizing.js";
import "./functions/smash.js";
import "./functions/sqrt.js";
import "./functions/styling.js";
import "./functions/supsub.js";
import "./functions/symbolsOp.js";
import "./functions/symbolsOrd.js";
import "./functions/symbolsSpacing.js";
import "./functions/tag.js";
import "./functions/text.js";
// import "./functions/tip.js";
// import "./functions/toggle.js";
import "./functions/vcenter.js";
import "./functions/verb.js";
