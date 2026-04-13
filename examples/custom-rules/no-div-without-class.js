// Example custom rule: require a class attribute on all <div> elements.
//
// Add to oxvelte.config.json:
//   { "customRules": ["./rules/no-div-without-class.js"] }

export default {
  name: "custom/no-div-without-class",

  run(ctx) {
    ctx.walk((node) => {
      if (node.type !== "Element" || node.name !== "div") return;

      const hasClass = node.attributes.some(
        (a) =>
          (a.type === "NormalAttribute" && a.name === "class") ||
          (a.type === "Directive" && a.kind === "Class"),
      );

      if (!hasClass) {
        ctx.diagnostic("div elements must have a class attribute", node.span);
      }
    });
  },
};
