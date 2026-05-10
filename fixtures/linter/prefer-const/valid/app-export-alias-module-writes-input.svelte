<script module>
	let shared = null;
	let moduleFlag = false;

	export function setModuleFlag() {
		moduleFlag = true;
	}
</script>

<script>
	let className = '';
	export { className as class };

	shared = {};
</script>

<div class={className}></div>
