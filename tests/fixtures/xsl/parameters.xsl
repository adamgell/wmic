<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
<xsl:output method="text"/><xsl:param name="greeting" select="'default'"/>
<xsl:template match="/"><xsl:value-of select="$greeting"/><xsl:text>|</xsl:text><xsl:value-of select="name(/*)"/><xsl:text>|</xsl:text><xsl:value-of select="//INSTANCE/@CLASSNAME"/><xsl:text>|</xsl:text><xsl:value-of select="//PROPERTY[@NAME='Caption']/VALUE"/></xsl:template>
</xsl:stylesheet>
