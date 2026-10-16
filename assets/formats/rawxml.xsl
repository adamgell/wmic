<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
<xsl:output method="xml" encoding="utf-16" indent="no" omit-xml-declaration="yes"/>
<xsl:template match="/"><xsl:apply-templates select="*"/></xsl:template>
<xsl:template match="*"><xsl:copy><xsl:copy-of select="@*"/><xsl:if test="@* and *"><xsl:text>&#13;&#10;</xsl:text></xsl:if><xsl:apply-templates select="node()"/></xsl:copy><xsl:text>&#13;&#10;</xsl:text></xsl:template>
<xsl:template match="text()|comment()|processing-instruction()"><xsl:copy-of select="."/></xsl:template>
</xsl:stylesheet>
