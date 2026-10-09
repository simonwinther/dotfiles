import { defineFunctionBuilders } from "../defineFunction.js";
import * as mathMLTree from "../mathMLTree.js";
import * as mml from "../buildMathML.js";

defineFunctionBuilders({
  type: "toggle",
  mathmlBuilder(group, style) {
    const expression = mml.buildExpression(group.body, style)
    const node = new mathMLTree.MathNode("maction", expression, [], { cursor: "default" })
    node.setAttribute("actiontype", "toggle")
    return node
  }
})
