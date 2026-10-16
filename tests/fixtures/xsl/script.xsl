<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" xmlns:msxsl="urn:schemas-microsoft-com:xslt" xmlns:test="urn:wmiccompat-test">
<xsl:output method="text"/>
<msxsl:script language="JScript" implements-prefix="test">function value() { return "script-ok"; }</msxsl:script>
<xsl:template match="/"><xsl:value-of select="test:value()"/></xsl:template>
</xsl:stylesheet>
