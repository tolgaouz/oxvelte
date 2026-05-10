<script lang="ts">
	let loadedUrls = $state(new Set<string>());
</script>

<img
	src="/image.png"
	alt=""
	onload={() => {
		loadedUrls = new Set(loadedUrls).add('/image.png');
	}}
/>
